use super::routing::quote_for_swap;
use super::schema::{ModelProfile, ModelSpec};
use std::collections::HashMap;
use std::path::Path;

pub fn split_cli_tokens(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else if ch == '\\' && q == '"' {
                    if let Some(&next) = chars.peek() {
                        if next == '"' || next == '\\' {
                            cur.push(chars.next().unwrap());
                        } else {
                            cur.push(ch);
                        }
                    } else {
                        cur.push(ch);
                    }
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn strip_model_prefix(raw: &str, models_dir: &str) -> String {
    let trimmed = raw.trim();
    for prefix in ["__MODELS__\\", "__MODELS__/"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    let mdir = models_dir.trim().trim_end_matches(['\\', '/']);
    if !mdir.is_empty()
        && let Some(rest) = trimmed.strip_prefix(mdir)
    {
        let rest = rest.trim_start_matches(['\\', '/']);
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    trimmed.to_string()
}

impl ModelSpec {
    pub fn effective_spec_type(&self) -> &str {
        let s = self.spec_type.trim();
        if !s.is_empty() { s } else if self.mtp_enabled { "draft-mtp" } else { "off" }
    }

    pub fn set_spec_type(&mut self, stype: &str) {
        self.spec_type = stype.to_string();
        self.mtp_enabled = stype != "off";
    }

    pub fn short_summary(&self) -> String {
        let ngl_txt = if self.ngl_mode == "auto" { "auto".to_string() } else { self.ngl_layers.to_string() };
        let offload_txt = if self.moe_enabled {
            let base = if self.all_moe_cpu { "MoE:ALL-CPU".to_string() } else { format!("MoE-CPU:{}", self.n_cpu_moe) };
            if self.moe_cache_mib > 0 {
                if self.moe_cache_cpu_share > 0.001 {
                    format!("{base} ({}MiB/{:.2})", self.moe_cache_mib, self.moe_cache_cpu_share)
                } else {
                    format!("{base} ({}MiB)", self.moe_cache_mib)
                }
            } else { base }
        } else if self.n_cpu_ffn > 0 {
            format!("FFN-CPU:{}", self.n_cpu_ffn)
        } else {
            "Standard".to_string()
        };
        let stype = self.effective_spec_type();
        let spec_txt = if stype != "off" {
            if self.mtp_draft_p_min > 0.001 {
                format!("{stype}:{}@{:.2}", self.mtp_draft_max, self.mtp_draft_p_min)
            } else {
                format!("{stype}:{}", self.mtp_draft_max)
            }
        } else {
            "NoSpec".to_string()
        };
        let r_txt = match self.reasoning_family.as_str() {
            "effort3" => format!("Effort:{}", self.reasoning_mode),
            "binary" => format!("Think:{}", self.reasoning_mode.to_uppercase()),
            _ => "Instruct".to_string(),
        };
        format!(
            "Ctx:{} | GPU:{} | {} | {} | {} | KV:{}/{}",
            self.ctx_size, ngl_txt, offload_txt, spec_txt, r_txt, self.cache_type_k, self.cache_type_v
        )
    }

    fn append_missing_sampler_args(&self, args: &mut Vec<String>) {
        if !self.use_custom_sampling {
            return;
        }
        let has = |flag: &str, list: &[String]| list.iter().any(|a| a == flag);
        if !has("--temp", args) {
            args.extend(["--temp".to_string(), format!("{:.2}", self.temp)]);
        }
        if !has("--min-p", args) {
            args.extend(["--min-p".to_string(), format!("{:.2}", self.min_p)]);
        }
        if !has("--top-p", args) {
            args.extend(["--top-p".to_string(), format!("{:.2}", self.top_p)]);
        }
        if !has("--top-k", args) {
            args.extend(["--top-k".to_string(), self.top_k.to_string()]);
        }
        if self.presence_penalty.abs() > 0.001 && !has("--presence-penalty", args) {
            args.extend(["--presence-penalty".to_string(), format!("{:.2}", self.presence_penalty)]);
        }
        if (self.repeat_penalty - 1.0).abs() > 0.001 && !has("--repeat-penalty", args) {
            args.extend(["--repeat-penalty".to_string(), format!("{:.2}", self.repeat_penalty)]);
        }
        if self.dry_multiplier > 0.001 {
            if !has("--dry-multiplier", args) {
                args.extend(["--dry-multiplier".to_string(), format!("{:.2}", self.dry_multiplier)]);
            }
            if !has("--dry-base", args) {
                args.extend(["--dry-base".to_string(), format!("{:.2}", self.dry_base)]);
            }
            if !has("--dry-allowed-length", args) {
                args.extend(["--dry-allowed-length".to_string(), self.dry_allowed_length.to_string()]);
            }
        }
    }

    pub fn legacy_rpc_opt(&self) -> Option<&str> {
        if self.use_rpc && !self.rpc_endpoint.trim().is_empty() {
            Some(self.rpc_endpoint.trim())
        } else {
            None
        }
    }

    pub fn build_cli_args_with_port(&self, models_dir: &str, host: &str, port_token: &str) -> Vec<String> {
        self.build_cli_args_with_port_and_rpc(models_dir, host, port_token, self.legacy_rpc_opt())
    }

    pub fn build_cli_args_with_port_and_rpc(
        &self,
        models_dir: &str,
        host: &str,
        port_token: &str,
        resolved_rpc: Option<&str>,
    ) -> Vec<String> {
        let full_model_path = Path::new(models_dir).join(&self.model_rel_path);
        let mut args = vec!["-m".to_string(), full_model_path.to_string_lossy().to_string()];

        if self.use_alias && !self.alias.trim().is_empty() {
            args.push("--alias".to_string());
            args.push(self.alias.trim().to_string());
        }

        args.extend([
            "-c".to_string(), self.ctx_size.to_string(),
            "-b".to_string(), self.batch_size.to_string(),
            "-ub".to_string(), self.ubatch_size.to_string(),
            "-ngl".to_string(), if self.ngl_mode == "auto" { "auto".to_string() } else { self.ngl_layers.to_string() },
            "-t".to_string(), self.threads.to_string(),
        ]);

        if self.threads_batch > 0 {
            args.extend(["-tb".to_string(), self.threads_batch.to_string()]);
        }

        args.extend([
            "-fa".to_string(), self.flash_attn.clone(),
            "-ctk".to_string(), self.cache_type_k.clone(),
            "-ctv".to_string(), self.cache_type_v.clone(),
            "--parallel".to_string(), self.parallel_slots.to_string(),
        ]);

        if self.no_kv_offload { args.push("-nkvo".to_string()); }
        if self.kv_unified { args.push("-kvu".to_string()); }
        if self.swa_full { args.push("--swa-full".to_string()); }
        if self.ctx_checkpoints > 0 {
            args.extend(["--ctx-checkpoints".to_string(), self.ctx_checkpoints.to_string()]);
        }
        if self.load_mode != "auto" {
            args.extend(["-lm".to_string(), self.load_mode.clone()]);
        }
        if self.fit_mode != "default" {
            args.extend(["-fit".to_string(), self.fit_mode.clone()]);
            if self.fit_mode == "on" && self.fit_target_mib != 1024 {
                args.extend(["--fit-target".to_string(), self.fit_target_mib.to_string()]);
            }
        }
        if self.jinja { args.push("--jinja".to_string()); }

        if self.moe_enabled {
            if self.all_moe_cpu {
                args.push("-cmoe".to_string());
            } else if self.n_cpu_moe > 0 {
                args.extend(["--n-cpu-moe".to_string(), self.n_cpu_moe.to_string()]);
            }
            if self.moe_cache_mib > 0 {
                args.extend(["--moe-cache-mib".to_string(), self.moe_cache_mib.to_string()]);
                if self.moe_cache_cpu_share > 0.001 {
                    args.extend([
                        "--moe-cache-cpu-share".to_string(),
                        format!("{:.2}", self.moe_cache_cpu_share),
                    ]);
                }
            }
        }
        if self.n_cpu_ffn > 0 {
            args.extend(["-ncffn".to_string(), self.n_cpu_ffn.to_string()]);
        }

        match self.reasoning_family.as_str() {
            "effort3" => match self.reasoning_mode.as_str() {
                "xhigh" => args.extend(["--chat-template-kwargs".to_string(), r#"{"reasoning_effort":"xhigh"}"#.to_string()]),
                "medium" => args.extend(["--chat-template-kwargs".to_string(), r#"{"reasoning_effort":"medium"}"#.to_string()]),
                "low" => args.extend(["--chat-template-kwargs".to_string(), r#"{"reasoning_effort":"low"}"#.to_string()]),
                "off" => args.extend([
                    "--chat-template-kwargs".to_string(), r#"{"enable_thinking":false}"#.to_string(),
                    "--reasoning-budget".to_string(), "0".to_string(),
                ]),
                _ => {}
            },
            "binary" => match self.reasoning_mode.as_str() {
                "on" => args.extend(["--chat-template-kwargs".to_string(), r#"{"enable_thinking":true}"#.to_string()]),
                "off" => args.extend([
                    "--chat-template-kwargs".to_string(), r#"{"enable_thinking":false}"#.to_string(),
                    "--reasoning-budget".to_string(), "0".to_string(),
                ]),
                _ => {}
            },
            _ => {}
        }

        if self.reasoning_family != "none" && self.reasoning_mode != "off" {
            if self.reasoning_format != "default" {
                args.extend(["--reasoning-format".to_string(), self.reasoning_format.clone()]);
            }
            if self.reasoning_preserve {
                args.push("--reasoning-preserve".to_string());
            }
        }

        let stype = self.effective_spec_type();
        if stype != "off" {
            args.extend([
                "--spec-type".to_string(), stype.to_string(),
                "--spec-draft-n-max".to_string(), self.mtp_draft_max.to_string(),
            ]);
            if self.mtp_draft_p_min > 0.001 {
                args.extend(["--spec-draft-p-min".to_string(), format!("{:.2}", self.mtp_draft_p_min)]);
            }
            if stype == "draft-mtp" && !self.mtp_draft_model_rel.trim().is_empty() {
                let draft_path = Path::new(models_dir).join(self.mtp_draft_model_rel.trim());
                args.push("--model-draft".to_string());
                args.push(draft_path.to_string_lossy().to_string());
            }
        }

        self.append_missing_sampler_args(&mut args);

        if let Some(ep) = resolved_rpc.map(str::trim).filter(|s| !s.is_empty()) {
            args.extend(["--rpc".to_string(), ep.to_string(), "--cache-ram".to_string(), "0".to_string()]);
        } else if self.cache_ram_mib >= 0 {
            args.extend(["--cache-ram".to_string(), self.cache_ram_mib.to_string()]);
        }

        if self.enable_metrics { args.push("--metrics".to_string()); }
        if self.verbosity >= 0 { args.extend(["-lv".to_string(), self.verbosity.to_string()]); }
        if self.server_timeout > 0 { args.extend(["--timeout".to_string(), self.server_timeout.to_string()]); }

        args.extend(["--host".to_string(), host.trim().to_string(), "--port".to_string(), port_token.to_string()]);
        for extra in split_cli_tokens(&self.extra_args) {
            args.push(extra);
        }
        args
    }

    pub fn build_template_flags_string(&self, models_dir: &str) -> String {
        let mut clone_no_samplers = self.clone();
        clone_no_samplers.use_custom_sampling = false;
        let full_model = Path::new(models_dir).join(&self.model_rel_path).to_string_lossy().to_string();
        let args = clone_no_samplers.build_cli_args_with_port(models_dir, "{host}", "{port}");
        let mut out = Vec::with_capacity(args.len());
        for tok in args {
            let replaced = if tok == full_model {
                "{model}".to_string()
            } else if tok == self.ctx_size.to_string() && out.last().map(|s: &String| s == "-c").unwrap_or(false) {
                "{ctx}".to_string()
            } else {
                tok
            };
            if replaced.contains(' ') || replaced.contains('"') {
                out.push(format!("'{replaced}'"));
            } else {
                out.push(replaced);
            }
        }
        out.join(" ")
    }

    pub fn expand_manual_args(&self, raw: &str, models_dir: &str, host: &str, port_token: &str) -> Vec<String> {
        let full_model = Path::new(models_dir).join(&self.model_rel_path).to_string_lossy().to_string();
        let ctx_s = self.ctx_size.to_string();
        let host_s = host.trim();
        let addr_s = format!("{host_s}:{port_token}");
        split_cli_tokens(raw)
            .into_iter()
            .map(|tok| {
                tok.replace("{model}", &full_model)
                    .replace("{ctx}", &ctx_s)
                    .replace("{host}", host_s)
                    .replace("{port}", port_token)
                    .replace("{addr}", &addr_s)
            })
            .collect()
    }

    pub fn build_ini_section(&self, models_dir: &str, fallback_name: &str) -> String {
        self.build_ini_section_with_rpc(models_dir, fallback_name, self.legacy_rpc_opt())
    }

    pub fn build_ini_section_with_rpc(
        &self,
        models_dir: &str,
        fallback_name: &str,
        resolved_rpc: Option<&str>,
    ) -> String {
        let sec_name = if !self.alias.trim().is_empty() {
            self.alias.trim()
        } else if !self.slot_name.trim().is_empty() {
            self.slot_name.trim()
        } else {
            fallback_name
        };
        let full_path = Path::new(models_dir).join(&self.model_rel_path);
        let mut lines = vec![
            format!("[{sec_name}]"),
            format!("model = {}", full_path.display()),
            format!("ctx-size = {}", self.ctx_size),
            format!("batch-size = {}", self.batch_size),
            format!("ubatch-size = {}", self.ubatch_size),
            format!("n-gpu-layers = {}", if self.ngl_mode == "auto" { "auto".to_string() } else { self.ngl_layers.to_string() }),
            format!("threads = {}", self.threads),
        ];
        if self.threads_batch > 0 { lines.push(format!("threads-batch = {}", self.threads_batch)); }
        lines.extend([
            format!("flash-attn = {}", self.flash_attn),
            format!("cache-type-k = {}", self.cache_type_k),
            format!("cache-type-v = {}", self.cache_type_v),
            format!("parallel = {}", self.parallel_slots),
            format!("jinja = {}", if self.jinja { "true" } else { "false" }),
        ]);

        if self.no_kv_offload { lines.push("no-kv-offload = true".to_string()); }
        if self.kv_unified { lines.push("kv-unified = true".to_string()); }
        if self.swa_full { lines.push("swa-full = true".to_string()); }
        if self.ctx_checkpoints > 0 { lines.push(format!("ctx-checkpoints = {}", self.ctx_checkpoints)); }
        if self.load_mode != "auto" { lines.push(format!("load-mode = {}", self.load_mode)); }
        if self.fit_mode != "default" {
            lines.push(format!("fit = {}", self.fit_mode));
            if self.fit_mode == "on" && self.fit_target_mib != 1024 {
                lines.push(format!("fit-target = {}", self.fit_target_mib));
            }
        }
        if self.moe_enabled {
            if self.all_moe_cpu { lines.push("cpu-moe = true".to_string()); }
            else if self.n_cpu_moe > 0 { lines.push(format!("n-cpu-moe = {}", self.n_cpu_moe)); }
            if self.moe_cache_mib > 0 {
                lines.push(format!("moe-cache-mib = {}", self.moe_cache_mib));
                if self.moe_cache_cpu_share > 0.001 {
                    lines.push(format!("moe-cache-cpu-share = {:.2}", self.moe_cache_cpu_share));
                }
            }
        }
        if self.n_cpu_ffn > 0 { lines.push(format!("n-cpu-ffn = {}", self.n_cpu_ffn)); }

        match self.reasoning_family.as_str() {
            "effort3" => match self.reasoning_mode.as_str() {
                "xhigh" => lines.push(r#"chat-template-kwargs = {"reasoning_effort":"xhigh"}"#.to_string()),
                "medium" => lines.push(r#"chat-template-kwargs = {"reasoning_effort":"medium"}"#.to_string()),
                "low" => lines.push(r#"chat-template-kwargs = {"reasoning_effort":"low"}"#.to_string()),
                "off" => {
                    lines.push(r#"chat-template-kwargs = {"enable_thinking":false}"#.to_string());
                    lines.push("reasoning-budget = 0".to_string());
                }
                _ => {}
            },
            "binary" => match self.reasoning_mode.as_str() {
                "on" => lines.push(r#"chat-template-kwargs = {"enable_thinking":true}"#.to_string()),
                "off" => {
                    lines.push(r#"chat-template-kwargs = {"enable_thinking":false}"#.to_string());
                    lines.push("reasoning-budget = 0".to_string());
                }
                _ => {}
            },
            _ => {}
        }

        if self.reasoning_family != "none" && self.reasoning_mode != "off" {
            if self.reasoning_format != "default" { lines.push(format!("reasoning-format = {}", self.reasoning_format)); }
            if self.reasoning_preserve { lines.push("reasoning-preserve = true".to_string()); }
        }

        let stype = self.effective_spec_type();
        if stype != "off" {
            lines.push(format!("spec-type = {stype}"));
            lines.push(format!("spec-draft-n-max = {}", self.mtp_draft_max));
            if self.mtp_draft_p_min > 0.001 { lines.push(format!("spec-draft-p-min = {:.2}", self.mtp_draft_p_min)); }
            if stype == "draft-mtp" && !self.mtp_draft_model_rel.trim().is_empty() {
                let draft_path = Path::new(models_dir).join(self.mtp_draft_model_rel.trim());
                lines.push(format!("model-draft = {}", draft_path.display()));
            }
        }

        if self.use_custom_sampling {
            lines.push(format!("temp = {:.2}", self.temp));
            lines.push(format!("min-p = {:.2}", self.min_p));
            lines.push(format!("top-p = {:.2}", self.top_p));
            lines.push(format!("top-k = {}", self.top_k));
            if self.presence_penalty.abs() > 0.001 { lines.push(format!("presence-penalty = {:.2}", self.presence_penalty)); }
            if (self.repeat_penalty - 1.0).abs() > 0.001 { lines.push(format!("repeat-penalty = {:.2}", self.repeat_penalty)); }
            if self.dry_multiplier > 0.001 {
                lines.push(format!("dry-multiplier = {:.2}", self.dry_multiplier));
                lines.push(format!("dry-base = {:.2}", self.dry_base));
                lines.push(format!("dry-allowed-length = {}", self.dry_allowed_length));
            }
        }

        if let Some(ep) = resolved_rpc.map(str::trim).filter(|s| !s.is_empty()) {
            lines.push(format!("rpc = {ep}"));
            lines.push("cache-ram = 0".to_string());
        } else if self.cache_ram_mib >= 0 {
            lines.push(format!("cache-ram = {}", self.cache_ram_mib));
        }

        let tokens = split_cli_tokens(&self.extra_args);
        let mut i = 0;
        while i < tokens.len() {
            let k = tokens[i].trim_start_matches('-');
            if i + 1 < tokens.len() && !tokens[i + 1].starts_with('-') {
                lines.push(format!("{} = {}", k, tokens[i + 1]));
                i += 2;
            } else {
                lines.push(format!("{} = true", k));
                i += 1;
            }
        }
        lines.join("\n")
    }
}

impl ModelProfile {


    pub fn seed_advanced_flags_from_simple(&self, models_dir: &str) -> String {
        if self.effective_engine_kind() == "custom" {
            self.single.extra_args.clone()
        } else {
            self.single.build_template_flags_string(models_dir)
        }
    }

    pub fn split_quoted_args(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if let Some(q) = quote {
                if q == '"' && ch == '\\' && chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            } else {
                match ch {
                    '"' | '\'' => quote = Some(ch),
                    c if c.is_whitespace() => {
                        if !cur.is_empty() {
                            out.push(std::mem::take(&mut cur));
                        }
                    }
                    c => cur.push(c),
                }
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    pub fn build_manual_args_with_port(&self, models_dir: &str, host: &str, port_token: &str) -> Vec<String> {
        self.build_manual_args_with_port_and_rpc(models_dir, host, port_token, self.single.legacy_rpc_opt())
    }

    pub fn build_manual_args_with_port_and_rpc(
        &self,
        models_dir: &str,
        host: &str,
        port_token: &str,
        resolved_rpc: Option<&str>,
    ) -> Vec<String> {
        let full_model = Path::new(models_dir).join(&self.single.model_rel_path).to_string_lossy().to_string();
        let ctx_s = self.single.ctx_size.to_string();
        let host_s = host.trim();
        let addr_s = format!("{host_s}:{port_token}");
        let raw_args: Vec<String> = Self::split_quoted_args(&self.manual_flags)
            .into_iter()
            .map(|tok| {
                tok.replace("{model}", &full_model)
                    .replace("{ctx}", &ctx_s)
                    .replace("{host}", host_s)
                    .replace("{port}", port_token)
                    .replace("{addr}", &addr_s)
            })
            .collect();

        let resolved_ep = resolved_rpc.map(str::trim).filter(|s| !s.is_empty());
        let mut args: Vec<String> = Vec::with_capacity(raw_args.len());
        let mut i = 0;
        while i < raw_args.len() {
            if raw_args[i] == "--rpc" && i + 1 < raw_args.len() {
                if let Some(ep) = resolved_ep {
                    args.push("--rpc".to_string());
                    args.push(ep.to_string());
                } else if !self.single.use_rpc {
                    args.push(raw_args[i].clone());
                    args.push(raw_args[i + 1].clone());
                }
                i += 2;
            } else if raw_args[i].starts_with("--rpc=") {
                if let Some(ep) = resolved_ep {
                    args.push(format!("--rpc={ep}"));
                } else if !self.single.use_rpc {
                    args.push(raw_args[i].clone());
                }
                i += 1;
            } else {
                args.push(raw_args[i].clone());
                i += 1;
            }
        }

        if self.single.use_custom_sampling && !args.iter().any(|a| a == "--temp") {
            args.extend([
                "--temp".to_string(), format!("{:.2}", self.single.temp),
                "--min-p".to_string(), format!("{:.2}", self.single.min_p),
                "--top-p".to_string(), format!("{:.2}", self.single.top_p),
                "--top-k".to_string(), self.single.top_k.to_string(),
            ]);
            if self.single.presence_penalty.abs() > 0.001 {
                args.extend(["--presence-penalty".to_string(), format!("{:.2}", self.single.presence_penalty)]);
            }
            if (self.single.repeat_penalty - 1.0).abs() > 0.001 {
                args.extend(["--repeat-penalty".to_string(), format!("{:.2}", self.single.repeat_penalty)]);
            }
            if self.single.dry_multiplier > 0.001 {
                args.extend([
                    "--dry-multiplier".to_string(), format!("{:.2}", self.single.dry_multiplier),
                    "--dry-base".to_string(), format!("{:.2}", self.single.dry_base),
                    "--dry-allowed-length".to_string(), self.single.dry_allowed_length.to_string(),
                ]);
            }
        }
        args
    }

    pub fn generate_manual_template(&self) -> String {
        self.generate_manual_template_with_rpc(self.single.legacy_rpc_opt())
    }

    pub fn generate_manual_template_with_rpc(&self, resolved_rpc: Option<&str>) -> String {
        if self.effective_engine_kind() == "custom" {
            return self.single.extra_args.clone();
        }
        let mut args =
            self.single
                .build_cli_args_with_port_and_rpc("__MODELS__", &self.host, "{port}", resolved_rpc);
        let mut i = 0;
        while i + 1 < args.len() {
            if args[i] == "-m" {
                args[i + 1] = "{model}".to_string();
            } else if args[i] == "-c" {
                args[i + 1] = "{ctx}".to_string();
            } else if args[i] == "--host" {
                args[i + 1] = "{host}".to_string();
            }
            i += 1;
        }
        args.into_iter().map(|a| quote_for_swap(&a)).collect::<Vec<_>>().join(" ")
    }

    pub fn sync_simple_from_manual_flags(&mut self, models_dir: &str) {
        if self.effective_engine_kind() == "custom" {
            self.single.extra_args = self.manual_flags.clone();
            return;
        }

        let tokens = Self::split_quoted_args(&self.manual_flags);
        let prev_reasoning_family = self.single.reasoning_family.clone();

        // Reset optional/toggleable Simple Mode fields so removed flags turn off cleanly.
        self.single.use_alias = false;
        self.single.threads_batch = 0;
        self.single.no_kv_offload = false;
        self.single.kv_unified = false;
        self.single.swa_full = false;
        self.single.ctx_checkpoints = 0;
        self.single.load_mode = "auto".to_string();
        self.single.fit_mode = "default".to_string();
        self.single.fit_target_mib = 1024;
        self.single.jinja = false;
        self.single.moe_enabled = false;
        self.single.all_moe_cpu = false;
        self.single.moe_cache_mib = 0;
        self.single.moe_cache_cpu_share = 0.0;
        self.single.n_cpu_ffn = 0;
        self.single.reasoning_preserve = false;
        self.single.mtp_draft_p_min = 0.0;
        self.single.mtp_draft_model_rel.clear();
        self.single.use_rpc = false;
        self.single.enable_metrics = false;
        self.single.verbosity = -1;
        self.single.server_timeout = 0;

        let mut saw_reasoning_mode = false;
        let mut parsed_reasoning_format: Option<String> = None;
        let mut parsed_reasoning_preserve = false;
        let mut parsed_spec_type: Option<String> = None;
        let mut parsed_model_draft: Option<String> = None;
        let mut parsed_cache_ram: Option<i32> = None;

        let mut saw_sampler = false;
        let mut saw_presence = false;
        let mut saw_repeat = false;
        let mut saw_dry_mult = false;

        let mut extra_tokens: Vec<String> = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            let tok = &tokens[i];
            let (flag_name, inline_val): (&str, Option<&str>) = if tok.starts_with('-') {
                if let Some((k, v)) = tok.split_once('=') {
                    (k, Some(v))
                } else {
                    (tok.as_str(), None)
                }
            } else {
                (tok.as_str(), None)
            };

            let val_opt: Option<(String, usize)> = if let Some(v) = inline_val {
                Some((v.to_string(), 1))
            } else if i + 1 < tokens.len() {
                Some((tokens[i + 1].clone(), 2))
            } else {
                None
            };

            match flag_name {
                "-m" | "--model" => {
                    if let Some((val, step)) = val_opt {
                        if val != "{model}" && !val.trim().is_empty() {
                            self.single.model_rel_path = strip_model_prefix(&val, models_dir);
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--alias" | "-a" => {
                    if let Some((val, step)) = val_opt {
                        self.single.use_alias = true;
                        self.single.alias = val;
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-c" | "--ctx-size" => {
                    if let Some((val, step)) = val_opt {
                        if val != "{ctx}"
                            && let Ok(n) = val.parse::<u32>()
                            && n >= 512
                        {
                            self.single.ctx_size = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-b" | "--batch-size" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>()
                            && n > 0
                        {
                            self.single.batch_size = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-ub" | "--ubatch-size" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>()
                            && n > 0
                        {
                            self.single.ubatch_size = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-ngl" | "--n-gpu-layers" | "--gpu-layers" => {
                    if let Some((val, step)) = val_opt {
                        if val.eq_ignore_ascii_case("auto") {
                            self.single.ngl_mode = "auto".to_string();
                        } else if val.eq_ignore_ascii_case("all") {
                            self.single.ngl_mode = "custom".to_string();
                            self.single.ngl_layers = 999;
                        } else if let Ok(n) = val.parse::<i32>() {
                            self.single.ngl_mode = "custom".to_string();
                            self.single.ngl_layers = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-t" | "--threads" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>()
                            && n > 0
                        {
                            self.single.threads = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-tb" | "--threads-batch" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.threads_batch = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-fa" | "--flash-attn" => {
                    if let Some(v) = inline_val {
                        match v.to_ascii_lowercase().as_str() {
                            "on" | "1" | "true" => self.single.flash_attn = "on".to_string(),
                            "off" | "0" | "false" => self.single.flash_attn = "off".to_string(),
                            "auto" => self.single.flash_attn = "auto".to_string(),
                            _ => extra_tokens.push(tok.clone()),
                        }
                        i += 1;
                    } else if i + 1 < tokens.len()
                        && matches!(
                            tokens[i + 1].to_ascii_lowercase().as_str(),
                            "on" | "off" | "auto" | "1" | "0"
                        )
                    {
                        match tokens[i + 1].to_ascii_lowercase().as_str() {
                            "on" | "1" => self.single.flash_attn = "on".to_string(),
                            "off" | "0" => self.single.flash_attn = "off".to_string(),
                            _ => self.single.flash_attn = "auto".to_string(),
                        }
                        i += 2;
                    } else {
                        self.single.flash_attn = "on".to_string();
                        i += 1;
                    }
                }
                "-ctk" | "--cache-type-k" => {
                    if let Some((val, step)) = val_opt {
                        self.single.cache_type_k = val;
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-ctv" | "--cache-type-v" => {
                    if let Some((val, step)) = val_opt {
                        self.single.cache_type_v = val;
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--parallel" | "-np" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>()
                            && n > 0
                        {
                            self.single.parallel_slots = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-nkvo" | "--no-kv-offload" => {
                    self.single.no_kv_offload = true;
                    i += 1;
                }
                "-kvu" | "--kv-unified" => {
                    self.single.kv_unified = true;
                    i += 1;
                }
                "--swa-full" => {
                    self.single.swa_full = true;
                    i += 1;
                }
                "--ctx-checkpoints" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.ctx_checkpoints = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-lm" | "--load-mode" => {
                    if let Some((val, step)) = val_opt {
                        self.single.load_mode = val;
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-fit" | "--fit" => {
                    if let Some((val, step)) = val_opt {
                        let lower = val.to_ascii_lowercase();
                        if matches!(lower.as_str(), "on" | "off" | "default") {
                            self.single.fit_mode = lower;
                        } else {
                            extra_tokens.push(tok.clone());
                            if step == 2 {
                                extra_tokens.push(val);
                            }
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--fit-target" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.fit_target_mib = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--jinja" => {
                    self.single.jinja = true;
                    i += 1;
                }
                "-cmoe" | "--cpu-moe" => {
                    self.single.moe_enabled = true;
                    self.single.all_moe_cpu = true;
                    i += 1;
                }
                "--n-cpu-moe" | "-ncmoe" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.moe_enabled = true;
                            self.single.all_moe_cpu = false;
                            self.single.n_cpu_moe = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--moe-cache-mib" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.moe_enabled = true;
                            self.single.moe_cache_mib = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--moe-cache-cpu-share" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            self.single.moe_enabled = true;
                            self.single.moe_cache_cpu_share = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "-ncffn" | "--n-cpu-ffn" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.n_cpu_ffn = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--chat-template-kwargs" => {
                    if let Some((val, step)) = val_opt {
                        let compact: String = val.chars().filter(|c| !c.is_whitespace()).collect();
                        match compact.as_str() {
                            r#"{"reasoning_effort":"xhigh"}"# | "{reasoning_effort:xhigh}" => {
                                self.single.reasoning_family = "effort3".to_string();
                                self.single.reasoning_mode = "xhigh".to_string();
                                saw_reasoning_mode = true;
                            }
                            r#"{"reasoning_effort":"medium"}"# | "{reasoning_effort:medium}" => {
                                self.single.reasoning_family = "effort3".to_string();
                                self.single.reasoning_mode = "medium".to_string();
                                saw_reasoning_mode = true;
                            }
                            r#"{"reasoning_effort":"low"}"# | "{reasoning_effort:low}" => {
                                self.single.reasoning_family = "effort3".to_string();
                                self.single.reasoning_mode = "low".to_string();
                                saw_reasoning_mode = true;
                            }
                            r#"{"enable_thinking":true}"# | "{enable_thinking:true}" => {
                                self.single.reasoning_family = "binary".to_string();
                                self.single.reasoning_mode = "on".to_string();
                                saw_reasoning_mode = true;
                            }
                            r#"{"enable_thinking":false}"# | "{enable_thinking:false}" => {
                                self.single.reasoning_family = if prev_reasoning_family == "effort3" {
                                    "effort3".to_string()
                                } else {
                                    "binary".to_string()
                                };
                                self.single.reasoning_mode = "off".to_string();
                                saw_reasoning_mode = true;
                            }
                            _ => {
                                extra_tokens.push(tok.clone());
                                if step == 2 {
                                    extra_tokens.push(val);
                                }
                            }
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--reasoning" => {
                    if let Some((val, step)) = val_opt {
                        match val.to_ascii_lowercase().as_str() {
                            "off" => {
                                self.single.reasoning_family = if prev_reasoning_family == "effort3" {
                                    "effort3".to_string()
                                } else {
                                    "binary".to_string()
                                };
                                self.single.reasoning_mode = "off".to_string();
                                saw_reasoning_mode = true;
                            }
                            "on" => {
                                self.single.reasoning_family = "binary".to_string();
                                self.single.reasoning_mode = "on".to_string();
                                saw_reasoning_mode = true;
                            }
                            _ => {
                                extra_tokens.push(tok.clone());
                                if step == 2 {
                                    extra_tokens.push(val);
                                }
                            }
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--reasoning-budget" => {
                    if let Some((val, step)) = val_opt {
                        if val.trim() == "0" {
                            if !saw_reasoning_mode {
                                self.single.reasoning_family = if prev_reasoning_family == "effort3" {
                                    "effort3".to_string()
                                } else {
                                    "binary".to_string()
                                };
                                self.single.reasoning_mode = "off".to_string();
                                saw_reasoning_mode = true;
                            }
                        } else {
                            extra_tokens.push(tok.clone());
                            if step == 2 {
                                extra_tokens.push(val);
                            }
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--reasoning-format" => {
                    if let Some((val, step)) = val_opt {
                        parsed_reasoning_format = Some(val);
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--reasoning-preserve" => {
                    parsed_reasoning_preserve = true;
                    i += 1;
                }
                "--spec-type" => {
                    if let Some((val, step)) = val_opt {
                        parsed_spec_type = Some(val);
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--spec-draft-n-max" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.mtp_draft_max = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--spec-draft-p-min" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            self.single.mtp_draft_p_min = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--model-draft" | "-md" => {
                    if let Some((val, step)) = val_opt {
                        parsed_model_draft = Some(val);
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--temp" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            self.single.temp = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--min-p" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            self.single.min_p = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--top-p" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            self.single.top_p = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--top-k" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<i32>() {
                            saw_sampler = true;
                            self.single.top_k = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--presence-penalty" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            saw_presence = true;
                            self.single.presence_penalty = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--repeat-penalty" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            saw_repeat = true;
                            self.single.repeat_penalty = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--dry-multiplier" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            saw_dry_mult = true;
                            self.single.dry_multiplier = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--dry-base" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(f) = val.parse::<f32>() {
                            saw_sampler = true;
                            self.single.dry_base = f;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--dry-allowed-length" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            saw_sampler = true;
                            self.single.dry_allowed_length = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--rpc" => {
                    if let Some((val, step)) = val_opt {
                        self.single.use_rpc = true;
                        if !val.trim().is_empty() {
                            self.single.rpc_endpoint = val;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--cache-ram" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<i32>() {
                            parsed_cache_ram = Some(n);
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--metrics" => {
                    self.single.enable_metrics = true;
                    i += 1;
                }
                "-lv" | "--log-verbosity" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<i32>() {
                            self.single.verbosity = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--timeout" | "-to" => {
                    if let Some((val, step)) = val_opt {
                        if let Ok(n) = val.parse::<u32>() {
                            self.single.server_timeout = n;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--host" => {
                    if let Some((val, step)) = val_opt {
                        if val != "{host}" && !val.trim().is_empty() {
                            self.host = val;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                "--port" => {
                    if let Some((val, step)) = val_opt {
                        if val != "{port}"
                            && let Ok(p) = val.parse::<u16>()
                            && p >= 1024
                        {
                            self.port = p;
                        }
                        i += step;
                    } else {
                        extra_tokens.push(tok.clone());
                        i += 1;
                    }
                }
                _ => {
                    extra_tokens.push(tok.clone());
                    i += 1;
                }
            }
        }

        // Finalize reasoning fields without dropping standalone flags.
        if saw_reasoning_mode {
            if self.single.reasoning_mode != "off" {
                self.single.reasoning_format =
                    parsed_reasoning_format.unwrap_or_else(|| "default".to_string());
                self.single.reasoning_preserve = parsed_reasoning_preserve;
            } else {
                if let Some(fmt) = parsed_reasoning_format {
                    extra_tokens.push("--reasoning-format".to_string());
                    extra_tokens.push(fmt);
                }
                if parsed_reasoning_preserve {
                    extra_tokens.push("--reasoning-preserve".to_string());
                }
            }
        } else {
            self.single.reasoning_family = "none".to_string();
            self.single.reasoning_mode = "off".to_string();
            if let Some(fmt) = parsed_reasoning_format {
                extra_tokens.push("--reasoning-format".to_string());
                extra_tokens.push(fmt);
            }
            if parsed_reasoning_preserve {
                extra_tokens.push("--reasoning-preserve".to_string());
            }
        }

        // Finalize speculative decoding / MTP fields.
        if let Some(st) = parsed_spec_type {
            self.single.set_spec_type(&st);
            if let Some(md) = parsed_model_draft {
                if st == "draft-mtp" {
                    self.single.mtp_draft_model_rel = strip_model_prefix(&md, models_dir);
                } else {
                    extra_tokens.push("--model-draft".to_string());
                    extra_tokens.push(md);
                }
            }
        } else {
            self.single.set_spec_type("off");
            self.single.mtp_draft_p_min = 0.0;
            self.single.mtp_draft_model_rel.clear();
            if let Some(md) = parsed_model_draft {
                extra_tokens.push("--model-draft".to_string());
                extra_tokens.push(md);
            }
        }

        // Finalize custom samplers.
        self.single.use_custom_sampling = saw_sampler;
        if saw_sampler {
            if !saw_presence {
                self.single.presence_penalty = 0.0;
            }
            if !saw_repeat {
                self.single.repeat_penalty = 1.0;
            }
            if !saw_dry_mult {
                self.single.dry_multiplier = 0.0;
            }
        }

        // Finalize cache-ram (treat --cache-ram 0 alongside --rpc as RPC's auto default).
        self.single.cache_ram_mib = match parsed_cache_ram {
            Some(0) if self.single.use_rpc => -1,
            Some(n) => n,
            None => -1,
        };

        self.single.extra_args = extra_tokens
            .into_iter()
            .map(|t| quote_for_swap(&t))
            .collect::<Vec<_>>()
            .join(" ");
    }

    pub fn build_args_with_port(&self, models_dir: &str, port_token: &str) -> Vec<String> {
        self.build_args_with_port_and_rpc(models_dir, port_token, self.single.legacy_rpc_opt())
    }

    pub fn build_args_with_port_and_rpc(
        &self,
        models_dir: &str,
        port_token: &str,
        resolved_rpc: Option<&str>,
    ) -> Vec<String> {
        if self.use_manual_flags && !self.manual_flags.trim().is_empty() {
            self.build_manual_args_with_port_and_rpc(models_dir, &self.host, port_token, resolved_rpc)
        } else if self.effective_engine_kind() == "custom" {
            {
                let full_model = Path::new(models_dir).join(&self.single.model_rel_path).to_string_lossy().to_string();
                let ctx_s = self.single.ctx_size.to_string();
                let host_s = self.host.trim();
                let addr_s = format!("{host_s}:{port_token}");
                Self::split_quoted_args(&self.single.extra_args)
                    .into_iter()
                    .map(|tok| {
                        tok.replace("{model}", &full_model)
                            .replace("{ctx}", &ctx_s)
                            .replace("{host}", host_s)
                            .replace("{port}", port_token)
                            .replace("{addr}", &addr_s)
                    })
                    .collect()
            }
        } else {
            self.single.build_cli_args_with_port_and_rpc(models_dir, &self.host, port_token, resolved_rpc)
        }
    }

    pub fn build_llama_args(&self, models_dir: &str) -> Vec<String> {
        self.build_args_with_port(models_dir, &self.port.to_string())
    }

    pub fn build_llama_args_with_rpc(&self, models_dir: &str, resolved_rpc: Option<&str>) -> Vec<String> {
        self.build_args_with_port_and_rpc(models_dir, &self.port.to_string(), resolved_rpc)
    }

    pub fn expand_custom_template(&self, template: &str, models_dir: &str, port_token: &str) -> Vec<String> {
        let full_model = Path::new(models_dir).join(&self.single.model_rel_path).to_string_lossy().to_string();
        let ctx_s = self.single.ctx_size.to_string();
        let host_s = self.host.trim();
        let addr_s = format!("{host_s}:{port_token}");
        Self::split_quoted_args(template)
            .into_iter()
            .map(|tok| {
                tok.replace("{model}", &full_model)
                    .replace("{ctx}", &ctx_s)
                    .replace("{host}", host_s)
                    .replace("{port}", port_token)
                    .replace("{addr}", &addr_s)
            })
            .collect()
    }





    pub fn code_text(&self, models_dir: &str) -> String {
        self.code_text_with_rpc(models_dir, self.single.legacy_rpc_opt())
    }

    pub fn code_text_with_rpc(&self, models_dir: &str, resolved_rpc: Option<&str>) -> String {
        let env_prefix = if self.should_disable_cuda_pinned() {
            "$env:GGML_CUDA_NO_PINNED = \"1\"\n"
        } else {
            ""
        };
        format!(
            "{env_prefix}{} {}",
            self.effective_engine_exe(),
            self.build_llama_args_with_rpc(models_dir, resolved_rpc).join(" ")
        )
    }
}

fn parse_cli_pairs(tokens: &[String]) -> (Vec<(String, String)>, HashMap<String, String>) {
    let mut ordered = Vec::new();
    let mut map = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.starts_with('-') {
            if i + 1 < tokens.len() && !tokens[i + 1].starts_with('-') {
                let val = tokens[i + 1].clone();
                ordered.push((tok.clone(), val.clone()));
                map.insert(tok.clone(), val);
                i += 2;
            } else {
                ordered.push((tok.clone(), String::new()));
                map.insert(tok.clone(), String::new());
                i += 1;
            }
        } else {
            ordered.push((tok.clone(), String::new()));
            i += 1;
        }
    }
    (ordered, map)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    Unchanged,
    Added,
    Modified,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliDiff {
    pub exe_name: String,
    pub env_pinned: Option<DiffKind>,
    pub tokens: Vec<(String, DiffKind)>,
    pub removed: Vec<String>,
    pub change_count: usize,
}

pub fn compute_cli_diff(current: &ModelProfile, saved: &ModelProfile, models_dir: &str) -> CliDiff {
    compute_cli_diff_with_rpc(
        current,
        saved,
        models_dir,
        current.single.legacy_rpc_opt(),
        saved.single.legacy_rpc_opt(),
    )
}

pub fn compute_cli_diff_with_rpc(
    current: &ModelProfile,
    saved: &ModelProfile,
    models_dir: &str,
    cur_rpc: Option<&str>,
    sav_rpc: Option<&str>,
) -> CliDiff {
    let mut removed = Vec::new();
    let mut change_count = 0usize;

    let env_pinned = if current.should_disable_cuda_pinned() {
        if !saved.should_disable_cuda_pinned() {
            change_count += 1;
            Some(DiffKind::Added)
        } else {
            Some(DiffKind::Modified)
        }
    } else {
        if saved.should_disable_cuda_pinned() {
            removed.push("GGML_CUDA_NO_PINNED=1".to_string());
            change_count += 1;
        }
        None
    };

    if current.engine_dir_override != saved.engine_dir_override
        || current.engine_kind != saved.engine_kind
        || current.engine_exe_override != saved.engine_exe_override
        || current.use_manual_flags != saved.use_manual_flags
        || current.use_advanced_flags != saved.use_advanced_flags
    {
        change_count += 1;
    }

    let cur_args = current.build_llama_args_with_rpc(models_dir, cur_rpc);
    let sav_args = saved.build_llama_args_with_rpc(models_dir, sav_rpc);
    let (cur_pairs, cur_map) = parse_cli_pairs(&cur_args);
    let (sav_pairs, sav_map) = parse_cli_pairs(&sav_args);

    let mut tokens = Vec::with_capacity(cur_pairs.len());
    for (k, v) in &cur_pairs {
        let pair_str = if v.is_empty() { k.clone() } else { format!("{k} {v}") };
        match sav_map.get(k) {
            None => {
                tokens.push((pair_str, DiffKind::Added));
                change_count += 1;
            }
            Some(old_v) if old_v != v => {
                tokens.push((pair_str, DiffKind::Modified));
                change_count += 1;
            }
            Some(_) => {
                tokens.push((pair_str, DiffKind::Unchanged));
            }
        }
    }

    for (k, v) in &sav_pairs {
        if !cur_map.contains_key(k) {
            let pair_str = if v.is_empty() { k.clone() } else { format!("{k} {v}") };
            removed.push(pair_str);
            change_count += 1;
        }
    }

    CliDiff {
        exe_name: current.effective_engine_exe().to_string(),
        env_pinned,
        tokens,
        removed,
        change_count,
    }
}
