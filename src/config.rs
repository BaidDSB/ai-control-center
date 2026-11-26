use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const CONFIG_FILE_V3: &str = "control_center_config_v3.toml";

fn default_svc_enabled() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct CompanionService {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub start_cmd: String,
    #[serde(default)]
    pub stop_cmd: String,
    #[serde(default)]
    pub check_port: u16,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_svc_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct QuickScript {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub path: String,
}
const LEGACY_V2_CFG: &str = "control_center_config.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    OledBlack,
    Grey,
    Light,
}

#[derive(Clone, Debug, Default)]
pub struct GgufInfo {
    pub valid: bool,
    pub file_size_gb: f64,
    pub arch: String,
    pub block_count: Option<u64>,
    pub context_length: Option<u64>,
    pub expert_count: Option<u64>,
}

fn read_u32_le<R: Read>(r: &mut R) -> Option<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b))
}

fn read_u64_le<R: Read>(r: &mut R) -> Option<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

fn read_gguf_str<R: Read>(r: &mut R) -> Option<String> {
    let len = read_u64_le(r)? as usize;
    if len > 4096 { return None; }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn skip_gguf_val<R: Read + Seek>(r: &mut R, vtype: u32) -> Option<()> {
    match vtype {
        0 | 1 | 7 => { r.seek(SeekFrom::Current(1)).ok()?; }
        2 | 3 => { r.seek(SeekFrom::Current(2)).ok()?; }
        4 | 5 | 6 => { r.seek(SeekFrom::Current(4)).ok()?; }
        10 | 11 | 12 => { r.seek(SeekFrom::Current(8)).ok()?; }
        8 => {
            let len = read_u64_le(r)? as i64;
            r.seek(SeekFrom::Current(len)).ok()?;
        }
        9 => {
            let elem_type = read_u32_le(r)?;
            let count = read_u64_le(r)?;
            if count > 200_000 { return None; }
            for _ in 0..count { skip_gguf_val(r, elem_type)?; }
        }
        _ => return None,
    }
    Some(())
}

pub fn probe_gguf_header(models_dir: &str, rel_path: &str) -> GgufInfo {
    let full = Path::new(models_dir).join(rel_path);
    let Ok(meta) = fs::metadata(&full) else { return GgufInfo::default(); };
    let size_gb = meta.len() as f64 / (1024.0 * 1024.0 * 1024.0);
    let Ok(file) = File::open(&full) else { return GgufInfo::default(); };
    let mut r = BufReader::new(file);
    let mut magic = [0u8; 4];
    if r.read_exact(&mut magic).is_err() || &magic != b"GGUF" { return GgufInfo::default(); }
    let Some(ver) = read_u32_le(&mut r) else { return GgufInfo::default(); };
    if !(2..=3).contains(&ver) { return GgufInfo::default(); }
    let Some(_t) = read_u64_le(&mut r) else { return GgufInfo::default(); };
    let Some(kv_cnt) = read_u64_le(&mut r) else { return GgufInfo::default(); };

    let mut info = GgufInfo {
        valid: true,
        file_size_gb: size_gb,
        arch: "unknown".to_string(),
        block_count: None,
        context_length: None,
        expert_count: None,
    };

    for _ in 0..kv_cnt.min(512) {
        let Some(key) = read_gguf_str(&mut r) else { break; };
        let Some(vtype) = read_u32_le(&mut r) else { break; };
        if key == "general.architecture" && vtype == 8 {
            if let Some(s) = read_gguf_str(&mut r) { info.arch = s; } else { break; }
        } else if key.ends_with(".block_count") && (vtype == 4 || vtype == 10) {
            info.block_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".context_length") && (vtype == 4 || vtype == 10) {
            info.context_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".expert_count") && (vtype == 4 || vtype == 10) {
            info.expert_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if skip_gguf_val(&mut r, vtype).is_none() {
            break;
        }
        if info.arch != "unknown"
            && info.block_count.is_some()
            && info.context_length.is_some()
            && info.expert_count.is_some()
        {
            break;
        }
    }
    info
}

fn default_fit_target() -> u32 { 1024 }
fn default_load_mode() -> String { "auto".to_string() }
fn default_cache_ram() -> i32 { -1 }
fn default_verbosity() -> i32 { -1 }
fn default_llama_exe() -> String { crate::platform::default_exe_name("llama-server") }
fn default_accent() -> [u8; 3] { [59, 130, 246] }
fn default_ttl() -> u32 { 600 }
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 8080 }
fn default_set_mode() -> String { "llama_swap".to_string() }

pub fn migrate_engine_path(p: &str) -> String {
    p.replace(r"\Llama.cpp\", r"\Engines\")
        .replace(r"\Llama.cpp", r"\Engines")
        .replace(r"\llama.cpp\", r"\Engines\")
}

pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn guess_help_key(name: &str, model_file: &str) -> String {
    let hay = format!("{} {}", name, model_file).to_ascii_lowercase();
    if hay.contains("3.6") || hay.contains("35b") { "qwen3.6-35b".to_string() }
    else if hay.contains("gemma") { "gemma4-26b".to_string() }
    else if hay.contains("3.8") || hay.contains("27b") { "qwen3.8-27b".to_string() }
    else if hay.contains("9b") || hay.contains("defiant") { "qwen3.5-9b".to_string() }
    else if hay.contains("4b") { "qwen3.5-4b".to_string() }
    else if hay.contains("cydonia") || hay.contains("magnum") { "cydonia-24b".to_string() }
    else if hay.contains("huihui") { "huihui-24b".to_string() }
    else { slugify(name) }
}

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EnginePreset {
    pub label: String,
    pub engine_kind: String, // "llama_cpp" | "custom"
    pub dir_path: String,
    #[serde(default = "default_llama_exe")]
    pub exe_name: String,
    #[serde(default)]
    pub color_rgb: Option<[u8; 3]>,
    #[serde(default)]
    pub default_args: String,
}

impl EnginePreset {
    pub fn default_exe_for_kind(_kind: &str) -> &'static str {
        #[cfg(windows)]
        {
            "llama-server.exe"
        }
        #[cfg(not(windows))]
        {
            "llama-server"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelSpec {
    pub slot_name: String,
    pub model_rel_path: String,
    pub use_alias: bool,
    pub alias: String,
    pub ctx_size: u32,
    pub ngl_mode: String,
    pub ngl_layers: i32,
    pub flash_attn: String,
    pub cache_type_k: String,
    pub cache_type_v: String,
    #[serde(default)]
    pub no_kv_offload: bool,
    #[serde(default)]
    pub kv_unified: bool,
    #[serde(default)]
    pub swa_full: bool,
    #[serde(default)]
    pub ctx_checkpoints: u32,
    #[serde(default = "default_cache_ram")]
    pub cache_ram_mib: i32,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub threads: u32,
    #[serde(default)]
    pub threads_batch: u32,
    pub parallel_slots: u32,
    pub jinja: bool,
    pub fit_mode: String,
    #[serde(default = "default_fit_target")]
    pub fit_target_mib: u32,
    #[serde(default = "default_load_mode")]
    pub load_mode: String,
    #[serde(default)]
    pub disable_cuda_pinned: bool,
    pub moe_enabled: bool,
    #[serde(default)]
    pub all_moe_cpu: bool,
    pub n_cpu_moe: u32,
    #[serde(default)]
    pub moe_cache_mib: u32,
    #[serde(default)]
    pub moe_cache_cpu_share: f32,
    #[serde(default)]
    pub n_cpu_ffn: u32,
    pub reasoning_family: String,
    pub reasoning_mode: String,
    pub reasoning_format: String,
    #[serde(default)]
    pub reasoning_preserve: bool,
    pub mtp_enabled: bool,
    #[serde(default)]
    pub spec_type: String,
    pub mtp_draft_max: u32,
    #[serde(default)]
    pub mtp_draft_p_min: f32,
    pub mtp_draft_model_rel: String,
    #[serde(default)]
    pub enable_metrics: bool,
    #[serde(default = "default_verbosity")]
    pub verbosity: i32,
    #[serde(default)]
    pub server_timeout: u32,
    pub use_custom_sampling: bool,
    pub temp: f32,
    pub min_p: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub presence_penalty: f32,
    pub repeat_penalty: f32,
    pub dry_multiplier: f32,
    pub dry_base: f32,
    pub dry_allowed_length: u32,
    pub use_rpc: bool,
    pub rpc_endpoint: String,
    pub extra_args: String,
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            slot_name: "Model-Slot".to_string(),
            model_rel_path: "Qwen3.5-4B-UD-Q4_K_XL.gguf".to_string(),
            use_alias: true,
            alias: "local-model".to_string(),
            ctx_size: 32768,
            ngl_mode: "custom".to_string(),
            ngl_layers: 999,
            flash_attn: "on".to_string(),
            cache_type_k: "q8_0".to_string(),
            cache_type_v: "q8_0".to_string(),
            no_kv_offload: false,
            kv_unified: false,
            swa_full: false,
            ctx_checkpoints: 0,
            cache_ram_mib: -1,
            batch_size: 2048,
            ubatch_size: 2048,
            threads: 6,
            threads_batch: 0,
            parallel_slots: 1,
            jinja: true,
            fit_mode: "default".to_string(),
            fit_target_mib: 1024,
            load_mode: "auto".to_string(),
            disable_cuda_pinned: false,
            moe_enabled: false,
            all_moe_cpu: false,
            n_cpu_moe: 40,
            moe_cache_mib: 0,
            moe_cache_cpu_share: 0.0,
            n_cpu_ffn: 0,
            reasoning_family: "binary".to_string(),
            reasoning_mode: "off".to_string(),
            reasoning_format: "deepseek".to_string(),
            reasoning_preserve: false,
            mtp_enabled: false,
            spec_type: String::new(),
            mtp_draft_max: 2,
            mtp_draft_p_min: 0.0,
            mtp_draft_model_rel: String::new(),
            enable_metrics: false,
            verbosity: -1,
            server_timeout: 0,
            use_custom_sampling: false,
            temp: 0.6,
            min_p: 0.0,
            top_p: 0.95,
            top_k: 20,
            presence_penalty: 0.0,
            repeat_penalty: 1.0,
            dry_multiplier: 0.0,
            dry_base: 1.75,
            dry_allowed_length: 2,
            use_rpc: false,
            rpc_endpoint: "127.0.0.1:50052".to_string(),
            extra_args: String::new(),
        }
    }
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

    pub fn build_cli_args_with_port(&self, models_dir: &str, host: &str, port_token: &str) -> Vec<String> {
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

        if self.use_rpc && !self.rpc_endpoint.trim().is_empty() {
            args.extend(["--rpc".to_string(), self.rpc_endpoint.trim().to_string(), "--cache-ram".to_string(), "0".to_string()]);
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

        if self.use_rpc && !self.rpc_endpoint.trim().is_empty() {
            lines.push(format!("rpc = {}", self.rpc_endpoint.trim()));
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelProfile {
    pub id: String,
    pub name: String,
    pub role_tag: String,
    pub packed_description: String,
    #[serde(default)]
    pub help_key: String,
    #[serde(default)]
    pub help_variant: String,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u32,
    #[serde(default)]
    pub engine_label: String,
    #[serde(default)]
    pub engine_kind: String, // "llama_cpp" | "custom"
    #[serde(default)]
    pub engine_dir_override: String,
    #[serde(default)]
    pub engine_exe_override: String,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub use_manual_flags: bool,
    #[serde(default)]
    pub manual_flags: String,
    #[serde(default)]
    pub use_advanced_flags: bool,
    #[serde(default)]
    pub advanced_flags: String,
    pub single: ModelSpec,
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self {
            id: "custom-model".to_string(),
            name: "Default Mode".to_string(),
            role_tag: "General Mode".to_string(),
            packed_description: "Custom GGUF profile.".to_string(),
            help_key: String::new(),
            help_variant: String::new(),
            ttl_seconds: 600,
            engine_label: String::new(),
            engine_kind: String::new(),
            engine_dir_override: String::new(),
            engine_exe_override: String::new(),
            host: "127.0.0.1".to_string(),
            port: 8080,
            use_manual_flags: false,
            manual_flags: String::new(),
            use_advanced_flags: false,
            advanced_flags: String::new(),
            single: ModelSpec::default(),
        }
    }
}

impl ModelProfile {
    pub fn effective_engine_kind(&self) -> &str {
        let k = self.engine_kind.trim();
        if k.is_empty() { "llama_cpp" } else { k }
    }

    pub fn effective_engine_dir<'a>(&'a self, global_llama_dir: &'a str) -> &'a str {
        let o = self.engine_dir_override.trim();
        if o.is_empty() { global_llama_dir } else { o }
    }

    pub fn effective_engine_exe(&self) -> &str {
        let o = self.engine_exe_override.trim();
        if !o.is_empty() { o } else { EnginePreset::default_exe_for_kind(self.effective_engine_kind()) }
    }

    pub fn engine_badge_name(&self) -> &str {
        let l = self.engine_label.trim();
        if !l.is_empty() { l } else if self.effective_engine_kind() == "custom" { self.effective_engine_exe() } else { "llama.cpp" }
    }

    pub fn help_engine_tag(&self) -> &'static str {
        let hay = format!(
            "{} {} {}",
            self.engine_label, self.engine_dir_override, self.engine_exe_override
        ).to_ascii_lowercase();
        if hay.contains("rusting") { "rustingstrata" }
        else if hay.contains("turbo") { "turboquant" }
        else { "llama.cpp" }
    }

    pub fn swap_model_id(&self) -> String {
        if self.single.use_alias && !self.single.alias.trim().is_empty() {
            self.single.alias.trim().to_string()
        } else if !self.id.trim().is_empty() {
            self.id.trim().to_string()
        } else {
            slugify(&self.name)
        }
    }

    pub fn should_disable_cuda_pinned(&self) -> bool {
        self.single.disable_cuda_pinned
    }

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
        for ch in raw.chars() {
            if let Some(q) = quote {
                if ch == q { quote = None; } else { cur.push(ch); }
            } else {
                match ch {
                    '"' | '\'' => quote = Some(ch),
                    c if c.is_whitespace() => {
                        if !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
                    }
                    c => cur.push(c),
                }
            }
        }
        if !cur.is_empty() { out.push(cur); }
        out
    }

    pub fn build_manual_args_with_port(&self, models_dir: &str, host: &str, port_token: &str) -> Vec<String> {
        let full_model = Path::new(models_dir).join(&self.single.model_rel_path).to_string_lossy().to_string();
        let ctx_s = self.single.ctx_size.to_string();
        let host_s = host.trim();
        let addr_s = format!("{host_s}:{port_token}");
        let mut args: Vec<String> = Self::split_quoted_args(&self.manual_flags)
            .into_iter()
            .map(|tok| {
                tok.replace("{model}", &full_model)
                    .replace("{ctx}", &ctx_s)
                    .replace("{host}", host_s)
                    .replace("{port}", port_token)
                    .replace("{addr}", &addr_s)
            })
            .collect();

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
        if self.effective_engine_kind() == "custom" {
            return self.single.extra_args.clone();
        }
        let mut args = self.single.build_cli_args_with_port("__MODELS__", &self.host, "{port}");
        let mut i = 0;
        while i + 1 < args.len() {
            if args[i] == "-m" { args[i + 1] = "{model}".to_string(); }
            else if args[i] == "-c" { args[i + 1] = "{ctx}".to_string(); }
            else if args[i] == "--host" { args[i + 1] = "{host}".to_string(); }
            i += 1;
        }
        args.into_iter().map(|a| quote_for_swap(&a)).collect::<Vec<_>>().join(" ")
    }

    pub fn build_args_with_port(&self, models_dir: &str, port_token: &str) -> Vec<String> {
        if self.use_manual_flags && !self.manual_flags.trim().is_empty() {
            self.build_manual_args_with_port(models_dir, &self.host, port_token)
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
            self.single.build_cli_args_with_port(models_dir, &self.host, port_token)
        }
    }

    pub fn build_llama_args(&self, models_dir: &str) -> Vec<String> {
        self.build_args_with_port(models_dir, &self.port.to_string())
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

    pub fn code_text(&self, models_dir: &str) -> String {
        let env_prefix = if self.should_disable_cuda_pinned() {
            "$env:GGML_CUDA_NO_PINNED = \"1\"\n"
        } else {
            ""
        };
        format!(
            "{env_prefix}{} {}",
            self.effective_engine_exe(),
            self.build_llama_args(models_dir).join(" ")
        )
    }
}

fn quote_for_swap(tok: &str) -> String {
    if tok.contains('"') || tok.contains(' ') {
        format!("\"{}\"", tok.replace('"', "\\\""))
    } else {
        tok.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelGroup {
    pub model_title: String,
    pub subtitle: String,
    pub profiles: Vec<ModelProfile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RoutingSet {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_set_mode")]
    pub mode: String, // "llama_swap" | "llama_ini"
    #[serde(default)]
    pub no_models_autoload: bool,
    pub profile_ids: Vec<String>,
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

pub fn compute_cli_diff(
    current: &ModelProfile,
    saved: &ModelProfile,
    models_dir: &str,
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

    let cur_args = current.build_llama_args(models_dir);
    let sav_args = saved.build_llama_args(models_dir);
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

pub type HelpMap = HashMap<(String, String, String), String>;

pub fn parse_help_markdown(path: &str) -> HelpMap {
    let Ok(content) = fs::read_to_string(path) else { return HashMap::new(); };
    parse_help_markdown_str(&content)
}

pub fn parse_help_markdown_str(content: &str) -> HelpMap {
    let mut map = HashMap::new();
    let mut current_key: Option<(String, String, String)> = None;
    let mut buf = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<!--") && trimmed.ends_with("-->") && trimmed.contains("@model:") {
            if let Some(k) = current_key.take() {
                map.insert(k, buf.join("\n").trim().to_string());
                buf.clear();
            }
            let inner = trimmed.trim_start_matches("<!--").trim_end_matches("-->").trim();
            let mut model = String::new();
            let mut engine = "general".to_string();
            let mut variant = String::new();
            for part in inner.split('|') {
                let p = part.trim();
                if let Some(v) = p.strip_prefix("@model:") { model = v.trim().to_ascii_lowercase(); }
                else if let Some(v) = p.strip_prefix("@engine:") { engine = v.trim().to_ascii_lowercase(); }
                else if let Some(v) = p.strip_prefix("@variant:") { variant = v.trim().to_ascii_lowercase(); }
            }
            if !model.is_empty() {
                current_key = Some((model, engine, variant));
            }
        } else if current_key.is_some() {
            buf.push(line);
        }
    }
    if let Some(k) = current_key.take() {
        map.insert(k, buf.join("\n").trim().to_string());
    }
    map
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StackConfig {
    pub app_title: String,
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_accent")]
    pub accent_rgb: [u8; 3],
    pub llama_dir: String,
    pub llama_swap_exe: String,
    pub llama_swap_yaml_path: String,
    pub help_md_path: String,
    #[serde(default)]
    pub saved_engines: Vec<EnginePreset>,
    pub models_dir: String,
    pub owu_scripts_dir: String,
    pub update_owu_bat: String,
    pub update_python_bat: String,
    pub favicon_path: String,
    pub router_ini_path: String,
    #[serde(default = "default_host")]
    pub server_host: String,
    #[serde(default = "default_port")]
    pub server_port: u16,
    pub owu_port: u16,
    #[serde(default)]
    pub frontend_name: String,
    #[serde(default)]
    pub frontend_exe: String,
    #[serde(default)]
    pub frontend_args: String,
    #[serde(default)]
    pub frontend_url: String,
    #[serde(default)]
    pub companion_services: Vec<CompanionService>,
    #[serde(default)]
    pub quick_scripts: Vec<QuickScript>,
    pub enable_mcpo_toggle: bool,
    pub mcpo_port: u16,
    pub mcpo_url: String,
    #[serde(default)]
    pub use_external_cmd_windows: bool,
    #[serde(default)]
    pub selected_set_idx: usize,
    #[serde(default)]
    pub selected_group_idx: usize,
    #[serde(default)]
    pub selected_profile_idx: usize,
    pub routing_sets: Vec<RoutingSet>,
    pub groups: Vec<ModelGroup>,
}

impl StackConfig {
    pub fn find_profile(&self, id: &str) -> Option<&ModelProfile> {
        for g in &self.groups {
            if let Some(p) = g.profiles.iter().find(|p| p.id == id) {
                return Some(p);
            }
        }
        None
    }

    pub fn find_profile_with_group(&self, id: &str) -> Option<(&ModelGroup, &ModelProfile)> {
        for g in &self.groups {
            if let Some(p) = g.profiles.iter().find(|p| p.id == id) {
                return Some((g, p));
            }
        }
        None
    }

    pub fn find_profile_pos(&self, id: &str) -> Option<(usize, usize)> {
        for (gi, g) in self.groups.iter().enumerate() {
            if let Some(pi) = g.profiles.iter().position(|p| p.id == id) {
                return Some((gi, pi));
            }
        }
        None
    }

    pub fn current_profile(&self) -> Option<&ModelProfile> {
        self.groups
            .get(self.selected_group_idx)
            .and_then(|g| g.profiles.get(self.selected_profile_idx))
    }

    pub fn make_unique_profile_id(&self, base_name: &str) -> String {
        let mut existing = HashSet::new();
        for g in &self.groups {
            for p in &g.profiles {
                existing.insert(p.id.clone());
            }
        }
        let mut base = slugify(base_name);
        if base.is_empty() {
            base = "profile".to_string();
        }
        if !existing.contains(&base) {
            return base;
        }
        let mut n = 2usize;
        loop {
            let cand = format!("{base}-{n}");
            if !existing.contains(&cand) {
                return cand;
            }
            n += 1;
        }
    }

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

    pub fn exe_dir() -> PathBuf {
        crate::platform::exe_dir()
    }

    pub fn config_path() -> PathBuf {
        crate::platform::resolve_config_dir(CONFIG_FILE_V3).join(CONFIG_FILE_V3)
    }

    pub fn resolve_config_relative(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() { return String::new(); }
        let p = Path::new(trimmed);
        if p.is_absolute() {
            trimmed.to_string()
        } else {
            let base = Self::config_path().parent().map(|d| d.to_path_buf()).unwrap_or_else(Self::exe_dir);
            base.join(p).to_string_lossy().to_string()
        }
    }

    pub fn resolved_llama_swap_yaml_path(&self) -> String {
        Self::resolve_config_relative(&self.llama_swap_yaml_path)
    }

    pub fn resolved_router_ini_path(&self) -> String {
        Self::resolve_config_relative(&self.router_ini_path)
    }

    pub fn effective_frontend_name(&self) -> &str {
        let n = self.frontend_name.trim();
        if !n.is_empty() {
            n
        } else if self.frontend_exe.to_ascii_lowercase().contains("webui") || self.owu_scripts_dir.to_ascii_lowercase().contains("webui") {
            "Open WebUI"
        } else {
            "Frontend"
        }
    }

    pub fn effective_frontend_exe_path(&self) -> PathBuf {
        let custom = self.frontend_exe.trim();
        if !custom.is_empty() {
            PathBuf::from(custom)
        } else {
            Path::new(&self.owu_scripts_dir).join(crate::platform::default_exe_name("open-webui"))
        }
    }

    pub fn effective_frontend_exe_name(&self) -> String {
        self.effective_frontend_exe_path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| crate::platform::default_exe_name("open-webui"))
    }

    pub fn effective_frontend_args(&self) -> Vec<String> {
        let port_s = self.owu_port.to_string();
        let raw = self.frontend_args.trim();
        if !raw.is_empty() {
            ModelProfile::split_quoted_args(raw)
                .into_iter()
                .map(|a| a.replace("{port}", &port_s))
                .collect()
        } else {
            vec!["serve".to_string(), "--port".to_string(), port_s]
        }
    }

    pub fn effective_frontend_url(&self) -> String {
        let port_s = self.owu_port.to_string();
        let raw = self.frontend_url.trim();
        if !raw.is_empty() {
            raw.replace("{port}", &port_s)
        } else {
            format!("http://127.0.0.1:{port_s}")
        }
    }

    pub fn load() -> Self {
        Self::load_with_hooks(None, None, None)
    }

    pub fn load_with_hooks(
        default_fn: Option<fn() -> StackConfig>,
        migrator_fn: Option<fn(&str) -> Result<StackConfig, String>>,
        post_load_fn: Option<fn(&mut StackConfig)>,
    ) -> Self {
        let v3_path = Self::config_path();
        let mut preserved_theme: Option<(ThemeMode, [u8; 3])> = None;

        if let Ok(raw) = fs::read_to_string(&v3_path) {
            if let Ok(mut cfg) = toml::from_str::<StackConfig>(&raw) {
                if !cfg.groups.is_empty() {
                    if cfg.selected_group_idx >= cfg.groups.len() { cfg.selected_group_idx = 0; }
                    let plen = cfg.groups[cfg.selected_group_idx].profiles.len();
                    if plen > 0 && cfg.selected_profile_idx >= plen { cfg.selected_profile_idx = 0; }
                    if !cfg.routing_sets.is_empty() && cfg.selected_set_idx >= cfg.routing_sets.len() {
                        cfg.selected_set_idx = 0;
                    }
                    if let Some(post) = post_load_fn { post(&mut cfg); }
                    return cfg;
                }
            } else {
                if let Ok(flat_meta) = toml::from_str::<FlatThemeHint>(&raw) {
                    preserved_theme = Some((flat_meta.theme_mode, flat_meta.accent_rgb));
                }
                let bak = Self::exe_dir().join("control_center_config_v3.flat.bak.toml");
                if !bak.exists() { let _ = fs::copy(&v3_path, &bak); }
            }
        }

        let v2_path = Self::exe_dir().join(LEGACY_V2_CFG);
        if let Ok(raw) = fs::read_to_string(&v2_path) {
            let res = match migrator_fn {
                Some(f) => f(&raw),
                None => migrate_from_v2(&raw),
            };
            if let Ok(mut migrated) = res {
                if let Some((tm, acc)) = preserved_theme {
                    migrated.theme_mode = tm;
                    migrated.accent_rgb = acc;
                }
                if let Some(post) = post_load_fn { post(&mut migrated); }
                let _ = migrated.save();
                return migrated;
            }
        }

        let mut def = match default_fn {
            Some(f) => f(),
            None => Self::default(),
        };
        if let Some((tm, acc)) = preserved_theme {
            def.theme_mode = tm;
            def.accent_rgb = acc;
        }
        if let Some(post) = post_load_fn { post(&mut def); }
        let _ = def.save();
        def
    }

    pub fn save(&self) -> Result<(), String> {
        let raw = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::config_path(), raw).map_err(|e| e.to_string())
    }
}

impl Default for StackConfig {
    fn default() -> Self {
        let llama_exe = crate::platform::default_exe_name("llama-server");
        let swap_exe = crate::platform::default_exe_name("llama-swap");

        let starter_profile = ModelProfile {
            id: "starter-model".to_string(),
            name: "Default Profile".to_string(),
            role_tag: "Starter Profile".to_string(),
            packed_description: "Enable Edit Mode to pick a GGUF file, configure engines, and add more profiles.".to_string(),
            engine_label: "llama.cpp".to_string(),
            engine_kind: "llama_cpp".to_string(),
            engine_dir_override: String::new(),
            engine_exe_override: llama_exe.clone(),
            ..ModelProfile::default()
        };

        let groups = vec![ModelGroup {
            model_title: "My First Model".to_string(),
            subtitle: "Starter Model Family (click Edit Mode to customize)".to_string(),
            profiles: vec![starter_profile],
        }];

        Self {
            app_title: "AI Control Center".to_string(),
            theme_mode: ThemeMode::OledBlack,
            accent_rgb: default_accent(),
            llama_dir: String::new(),
            llama_swap_exe: swap_exe,
            llama_swap_yaml_path: "llama-swap.yaml".to_string(),
            help_md_path: "help.md".to_string(),
            saved_engines: vec![EnginePreset {
                label: "llama.cpp".to_string(),
                engine_kind: "llama_cpp".to_string(),
                dir_path: String::new(),
                exe_name: llama_exe,
                color_rgb: Some([59, 130, 246]),
                default_args: String::new(),
            }],
            models_dir: String::new(),
            owu_scripts_dir: String::new(),
            update_owu_bat: String::new(),
            update_python_bat: String::new(),
            favicon_path: String::new(),
            router_ini_path: "models.ini".to_string(),
            server_host: "127.0.0.1".to_string(),
            server_port: 8080,
            owu_port: 3000,
            frontend_name: String::new(),
            frontend_exe: String::new(),
            frontend_args: String::new(),
            frontend_url: "http://127.0.0.1:{port}".to_string(),
            companion_services: Vec::new(),
            quick_scripts: Vec::new(),
            enable_mcpo_toggle: false,
            mcpo_port: 8000,
            mcpo_url: String::new(),
            use_external_cmd_windows: false,
            selected_set_idx: 0,
            selected_group_idx: 0,
            selected_profile_idx: 0,
            routing_sets: vec![RoutingSet {
                id: "default-set".to_string(),
                name: "Default Set".to_string(),
                description: "Starter Routing Set.".to_string(),
                mode: "llama_swap".to_string(),
                no_models_autoload: false,
                profile_ids: vec!["starter-model".to_string()],
            }],
            groups,
        }
    }
}

#[derive(Deserialize)]
struct FlatThemeHint {
    #[serde(default)]
    theme_mode: ThemeMode,
    #[serde(default = "default_accent")]
    accent_rgb: [u8; 3],
}

#[derive(Deserialize, Default)]
struct LegacyV2Profile {
    #[serde(default)] name: String,
    #[serde(default)] role_tag: String,
    #[serde(default)] packed_description: String,
    #[serde(default)] is_router_mode: bool,
    #[serde(default)] engine_label: String,
    #[serde(default)] engine_kind: String,
    #[serde(default)] engine_dir_override: String,
    #[serde(default)] engine_exe_override: String,
    #[serde(default = "default_host")] host: String,
    #[serde(default = "default_port")] port: u16,
    #[serde(default)] single: ModelSpec,
}

#[derive(Deserialize, Default)]
struct LegacyV2Group {
    #[serde(default)] model_title: String,
    #[serde(default)] subtitle: String,
    #[serde(default)] profiles: Vec<LegacyV2Profile>,
}

#[derive(Deserialize, Default)]
struct LegacyV2Config {
    #[serde(default)] app_title: String,
    #[serde(default)] llama_dir: String,
    #[serde(default)] saved_engines: Vec<EnginePreset>,
    #[serde(default)] models_dir: String,
    #[serde(default)] owu_scripts_dir: String,
    #[serde(default)] update_owu_bat: String,
    #[serde(default)] update_python_bat: String,
    #[serde(default)] favicon_path: String,
    #[serde(default)] router_ini_path: String,
    #[serde(default)] owu_port: u16,
    #[serde(default)] enable_mcpo_toggle: bool,
    #[serde(default)] mcpo_port: u16,
    #[serde(default)] mcpo_url: String,
    #[serde(default)] use_external_cmd_windows: bool,
    #[serde(default)] groups: Vec<LegacyV2Group>,
}

fn migrate_from_v2(raw: &str) -> Result<StackConfig, String> {
    let old: LegacyV2Config = toml::from_str(raw).map_err(|e| e.to_string())?;
    if old.groups.is_empty() {
        return Err("Empty v2 config".to_string());
    }
    let mut cfg = StackConfig::default();
    if !old.app_title.is_empty() { cfg.app_title = old.app_title; }
    if !old.llama_dir.is_empty() { cfg.llama_dir = migrate_engine_path(&old.llama_dir); }
    if !old.models_dir.is_empty() { cfg.models_dir = old.models_dir; }
    if !old.owu_scripts_dir.is_empty() { cfg.owu_scripts_dir = old.owu_scripts_dir; }
    if !old.update_owu_bat.is_empty() { cfg.update_owu_bat = old.update_owu_bat; }
    if !old.update_python_bat.is_empty() { cfg.update_python_bat = old.update_python_bat; }
    if !old.favicon_path.is_empty() { cfg.favicon_path = old.favicon_path; }
    if !old.router_ini_path.is_empty() { cfg.router_ini_path = old.router_ini_path; }
    if old.owu_port > 0 { cfg.owu_port = old.owu_port; }
    cfg.enable_mcpo_toggle = old.enable_mcpo_toggle;
    if old.mcpo_port > 0 { cfg.mcpo_port = old.mcpo_port; }
    if !old.mcpo_url.is_empty() { cfg.mcpo_url = old.mcpo_url; }
    cfg.use_external_cmd_windows = old.use_external_cmd_windows;

    if !old.saved_engines.is_empty() {
        cfg.saved_engines = old
            .saved_engines
            .into_iter()
            .map(|mut e| {
                e.dir_path = migrate_engine_path(&e.dir_path);
                e
            })
            .collect();
    }

    let mut migrated_groups = Vec::new();
    let mut seen_ids = HashSet::new();

    for g in old.groups {
        let mut sub_profiles = Vec::new();
        for p in &g.profiles {
            if p.is_router_mode { continue; }
            let full_label = format!("{} {}", g.model_title, p.name);
            let mut base_id = slugify(&full_label);
            if base_id.is_empty() { base_id = format!("profile-{}", seen_ids.len() + 1); }
            let mut id = base_id.clone();
            let mut n = 2;
            while seen_ids.contains(&id) {
                id = format!("{base_id}-{n}");
                n += 1;
            }
            seen_ids.insert(id.clone());
            let help_key = guess_help_key(&full_label, &p.single.model_rel_path);

            sub_profiles.push(ModelProfile {
                id,
                name: p.name.clone(),
                role_tag: p.role_tag.clone(),
                packed_description: p.packed_description.clone(),
                help_key,
                help_variant: String::new(),
                ttl_seconds: 600,
                engine_label: p.engine_label.clone(),
                engine_kind: p.engine_kind.clone(),
                engine_dir_override: migrate_engine_path(&p.engine_dir_override),
                engine_exe_override: p.engine_exe_override.clone(),
                host: p.host.clone(),
                port: p.port,
                use_manual_flags: false,
                manual_flags: String::new(),
                use_advanced_flags: false,
                advanced_flags: String::new(),
                single: p.single.clone(),
            });
        }

        if !sub_profiles.is_empty() {
            migrated_groups.push(ModelGroup {
                model_title: g.model_title,
                subtitle: g.subtitle,
                profiles: sub_profiles,
            });
        }
    }

    if !migrated_groups.is_empty() {
        cfg.groups = migrated_groups;
    }
    Ok(cfg)
}

pub fn scan_gguf_models(root: &str) -> Vec<String> {
    let base = Path::new(root);
    let mut found = Vec::new();
    if !base.exists() { return found; }
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(ext) = p.extension() {
                    if ext.eq_ignore_ascii_case("gguf") {
                        if let Ok(rel) = p.strip_prefix(base) {
                            found.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_gguf_str(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(&(s.len() as u64).to_le_bytes());
        buf.extend_from_slice(s.as_bytes());
    }

    #[test]
    fn default_exe_for_kind_matches_target_os() {
        let exe = EnginePreset::default_exe_for_kind("llama_cpp");
        if cfg!(windows) {
            assert_eq!(exe, "llama-server.exe");
        } else {
            assert_eq!(exe, "llama-server");
        }
    }

    #[test]
    fn cli_args_rewrite_port_and_handle_quoted_manual_flags() {
        let mut prof = ModelProfile::default();
        prof.use_manual_flags = true;
        prof.manual_flags =
            "-m {model} -c {ctx} --chat-template-file \"C:\\My Templates\\qwen.jinja\" --host {host} --port {port}"
                .to_string();
        prof.single.model_rel_path = "test-model.gguf".to_string();
        prof.single.ctx_size = 32768;

        let args = prof.build_args_with_port("D:\\Models", "${PORT}");
        assert!(args.iter().any(|a| a == "${PORT}"));
        assert!(args.iter().any(|a| a == "32768"));
        assert!(args.iter().any(|a| a.contains("My Templates")));
    }

    #[test]
    fn compute_cli_diff_detects_env_flag_and_manual_mode_changes() {
        let saved = ModelProfile::default();
        let mut current = saved.clone();

        let clean = compute_cli_diff(&current, &saved, "D:\\Models");
        assert_eq!(clean.change_count, 0);
        assert!(clean.removed.is_empty());

        current.single.ctx_size = saved.single.ctx_size + 8192;
        current.single.disable_cuda_pinned = true;
        current.use_manual_flags = true;
        let changed = compute_cli_diff(&current, &saved, "D:\\Models");
        assert!(changed.change_count >= 2);
        assert_eq!(changed.env_pinned, Some(DiffKind::Added));
    }

    #[test]
    fn routing_set_yaml_uses_cargo_version_and_swap_port() {
        let cfg = StackConfig::default();
        let set = cfg.routing_sets[0].clone();
        let yaml = cfg.build_routing_set_yaml(&set);

        let expected_banner = format!(
            "# Auto-generated by AI Control Center v{}",
            env!("CARGO_PKG_VERSION")
        );
        assert!(yaml.contains(&expected_banner));
        assert!(yaml.contains("models:"));
        assert!(yaml.contains("${PORT}"));
    }

    #[test]
    fn routing_set_ini_respects_use_manual_flags() {
        let mut cfg = StackConfig::default();
        let pid = cfg.groups[0].profiles[0].id.clone();
        {
            let prof = &mut cfg.groups[0].profiles[0];
            prof.use_manual_flags = true;
            prof.use_advanced_flags = false;
            prof.manual_flags = "-m {model} -c 65536 --host {host} --port {port}".to_string();
        }
        let set = RoutingSet {
            id: "ini-test".to_string(),
            name: "INI Test".to_string(),
            description: String::new(),
            mode: "llama_ini".to_string(),
            no_models_autoload: false,
            profile_ids: vec![pid],
        };

        let ini = cfg.build_routing_set_ini(&set);
        assert!(ini.contains("65536"));
        assert!(!ini.contains("--port"));
    }

    #[test]
    fn help_markdown_parser_extracts_model_sections() {
        assert!(parse_help_markdown_str("Plain text without tags").is_empty());

        let md = "<!-- @model: demo-model -->\nUse flash attention for speed.\n";
        let map = parse_help_markdown_str(md);
        let note = map
            .get(&(
                "demo-model".to_string(),
                "general".to_string(),
                String::new(),
            ))
            .expect("expected parsed demo-model general note");
        assert!(note.contains("Use flash attention for speed."));
    }

    #[test]
    fn probe_gguf_header_handles_valid_bad_magic_and_truncated_files() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "acc_gguf_test_{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&tmp_dir);
        let dir_str = tmp_dir.to_string_lossy().to_string();

        // 1. Minimal valid GGUF v3 header (arch = qwen2, 40 layers, 131072 ctx)
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GGUF");
        bytes.extend_from_slice(&3u32.to_le_bytes()); // version
        bytes.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        bytes.extend_from_slice(&3u64.to_le_bytes()); // metadata_kv_count

        write_gguf_str(&mut bytes, "general.architecture");
        bytes.extend_from_slice(&8u32.to_le_bytes()); // GGUF_TYPE_STRING
        write_gguf_str(&mut bytes, "qwen2");

        write_gguf_str(&mut bytes, "qwen2.block_count");
        bytes.extend_from_slice(&4u32.to_le_bytes()); // GGUF_TYPE_UINT32
        bytes.extend_from_slice(&40u32.to_le_bytes());

        write_gguf_str(&mut bytes, "qwen2.context_length");
        bytes.extend_from_slice(&4u32.to_le_bytes()); // GGUF_TYPE_UINT32
        bytes.extend_from_slice(&131072u32.to_le_bytes());

        let valid_path = tmp_dir.join("valid.gguf");
        fs::write(&valid_path, &bytes).expect("write valid gguf");
        let valid_info = probe_gguf_header(&dir_str, "valid.gguf");
        assert!(valid_info.valid);
        assert_eq!(valid_info.arch, "qwen2");
        assert_eq!(valid_info.block_count, Some(40));
        assert_eq!(valid_info.context_length, Some(131072));

        // 2. Bad magic
        let bad_path = tmp_dir.join("bad.gguf");
        fs::write(&bad_path, b"NOT_A_GGUF_HEADER_AT_ALL").expect("write bad gguf");
        assert!(!probe_gguf_header(&dir_str, "bad.gguf").valid);

        // 3. Truncated header
        let trunc_path = tmp_dir.join("trunc.gguf");
        fs::write(&trunc_path, b"GGUF\x03").expect("write trunc gguf");
        assert!(!probe_gguf_header(&dir_str, "trunc.gguf").valid);

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn slugify_normalizes_spaces_and_case() {
        assert_eq!(slugify("My Test Set 1"), "my-test-set-1");
    }
}