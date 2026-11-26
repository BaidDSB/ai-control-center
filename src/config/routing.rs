use super::schema::{EnginePreset, ModelProfile, RoutingSet, StackConfig};
use std::collections::HashSet;
use std::path::Path;

impl ModelProfile {
    fn ensure_swap_port_in_args(args: &mut Vec<String>, host: &str) {
        if args.is_empty() || args.iter().any(|a| a.contains("${PORT}")) {
            return;
        }
        let mut patched = false;
        let mut i = 0;
        while i < args.len() {
            let tok = args[i].as_str();
            if matches!(tok, "--port" | "-p") && i + 1 < args.len() {
                args[i + 1] = "${PORT}".to_string();
                patched = true;
                break;
            }
            if let Some(prefix) = ["--port=", "-p="].iter().find(|p| tok.starts_with(*p)) {
                args[i] = format!("{prefix}${{PORT}}");
                patched = true;
                break;
            }
            if matches!(tok, "--addr" | "--listen" | "--bind") && i + 1 < args.len() {
                let h = args[i + 1].rsplit_once(':').map(|(a, _)| a).unwrap_or(host);
                args[i + 1] = format!("{h}:${{PORT}}");
                patched = true;
                break;
            }
            if let Some(prefix) = ["--addr=", "--listen=", "--bind="].iter().find(|p| tok.starts_with(*p)) {
                let val = &tok[prefix.len()..];
                let h = val.rsplit_once(':').map(|(a, _)| a).unwrap_or(host);
                args[i] = format!("{prefix}{h}:${{PORT}}");
                patched = true;
                break;
            }
            i += 1;
        }
        if !patched {
            let h = if host.trim().is_empty() { "127.0.0.1" } else { host.trim() };
            if args.first().map(|s| s.eq_ignore_ascii_case("serve")).unwrap_or(false) {
                args.push("--addr".to_string());
                args.push(format!("{h}:${{PORT}}"));
            } else {
                args.push("--port".to_string());
                args.push("${PORT}".to_string());
            }
        }
    }

    pub fn build_swap_cmd_and_env_with_engines(
        &self,
        models_dir: &str,
        global_llama_dir: &str,
        saved_engines: &[EnginePreset],
    ) -> (String, Vec<String>) {
        let exe = Path::new(self.effective_engine_dir(global_llama_dir)).join(self.effective_engine_exe());
        let exe_s = quote_for_swap(&exe.to_string_lossy());
        let mut args = self.build_args_with_port(models_dir, "${PORT}");

        if args.is_empty() && self.effective_engine_kind() == "custom" {
            let lbl = self.engine_label.trim();
            let ex = self.effective_engine_exe();
            if let Some(eng) = saved_engines.iter().find(|e| {
                (!lbl.is_empty() && e.label.eq_ignore_ascii_case(lbl)) || e.exe_name.eq_ignore_ascii_case(ex)
            }) {
                if !eng.default_args.trim().is_empty() {
                    args = self.expand_custom_template(&eng.default_args, models_dir, "${PORT}");
                }
            }
        }
        Self::ensure_swap_port_in_args(&mut args, &self.host);

        let mut env_vars = Vec::new();
        if self.should_disable_cuda_pinned() {
            env_vars.push("GGML_CUDA_NO_PINNED=1".to_string());
        }
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(exe_s);
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--chat-template-kwargs" && i + 1 < args.len() {
                env_vars.push(format!("LLAMA_ARG_CHAT_TEMPLATE_KWARGS={}", args[i + 1]));
                i += 2;
            } else {
                parts.push(quote_for_swap(&args[i]));
                i += 1;
            }
        }
        (parts.join(" "), env_vars)
    }

    pub fn build_swap_cmd_and_env(&self, models_dir: &str, global_llama_dir: &str) -> (String, Vec<String>) {
        let exe = Path::new(self.effective_engine_dir(global_llama_dir)).join(self.effective_engine_exe());
        let exe_s = quote_for_swap(&exe.to_string_lossy());
        let args = self.build_args_with_port(models_dir, "${PORT}");

        let mut env_vars = Vec::new();
        if self.should_disable_cuda_pinned() {
            env_vars.push("GGML_CUDA_NO_PINNED=1".to_string());
        }
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(exe_s);
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--chat-template-kwargs" && i + 1 < args.len() {
                env_vars.push(format!("LLAMA_ARG_CHAT_TEMPLATE_KWARGS={}", args[i + 1]));
                i += 2;
            } else {
                parts.push(quote_for_swap(&args[i]));
                i += 1;
            }
        }
        (parts.join(" "), env_vars)
    }
}

pub(crate) fn quote_for_swap(tok: &str) -> String {
    if tok.contains('"') || tok.contains(' ') {
        format!("\"{}\"", tok.replace('"', "\\\""))
    } else {
        tok.to_string()
    }
} impl StackConfig {
    pub fn build_routing_set_yaml(&self, set: &RoutingSet) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Auto-generated by AI Control Center v{}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!("# Routing Set: {}\n", set.name));
        out.push_str("healthCheckTimeout: 180\nlogToStdout: \"both\"\n\nmodels:\n");

        let mut used_names: HashSet<String> = HashSet::new();
        for pid in &set.profile_ids {
            let Some(prof) = self.find_profile(pid) else { continue; };
            let mut key = prof.swap_model_id();
            if used_names.contains(&key) {
                key = prof.id.clone();
            }
            used_names.insert(key.clone());

            let (cmd, env_vars) = prof.build_swap_cmd_and_env_with_engines(&self.models_dir, &self.llama_dir, &self.saved_engines);
            out.push_str(&format!("  \"{}\":\n", key));
            if prof.ttl_seconds > 0 {
                out.push_str(&format!("    ttl: {}\n", prof.ttl_seconds));
            }
            if !env_vars.is_empty() {
                out.push_str("    env:\n");
                for ev in env_vars {
                    let safe = ev.replace('\'', "''");
                    out.push_str(&format!("      - '{safe}'\n"));
                }
            }
            out.push_str("    cmd: |\n");
            out.push_str(&format!("      {}\n\n", cmd));
        }
        out
    }

    pub fn build_routing_set_ini(&self, set: &RoutingSet) -> String {
        let mut out = String::from("[*]\nflash-attn = on\nthreads = 6\nparallel = 1\njinja = true\n\n");
        for pid in &set.profile_ids {
            if let Some(prof) = self.find_profile(pid) {
                if prof.effective_engine_kind() != "custom" {
                    if (prof.use_manual_flags && !prof.manual_flags.trim().is_empty()) || prof.use_advanced_flags {
                        let sec_name = prof.swap_model_id();
                        out.push_str(&format!("[{sec_name}]\n"));
                        let toks = prof.build_args_with_port(&self.models_dir, "8080");
                        let mut i = 0;
                        while i < toks.len() {
                            let flag = toks[i].as_str();
                            if flag == "--host" || flag == "--port" || flag == "--alias" {
                                i += 2;
                                continue;
                            }
                            if flag == "-m" && i + 1 < toks.len() {
                                out.push_str(&format!("model = {}\n", toks[i + 1]));
                                i += 2;
                            } else if flag.starts_with('-') {
                                let k = flag.trim_start_matches('-');
                                if i + 1 < toks.len() && !toks[i + 1].starts_with('-') {
                                    out.push_str(&format!("{k} = {}\n", toks[i + 1]));
                                    i += 2;
                                } else {
                                    out.push_str(&format!("{k} = true\n"));
                                    i += 1;
                                }
                            } else {
                                i += 1;
                            }
                        }
                        out.push('\n');
                    } else {
                        out.push_str(&prof.single.build_ini_section(&self.models_dir, &prof.id));
                        out.push_str("\n\n");
                    }
                }
            }
        }
        out
    }
}
