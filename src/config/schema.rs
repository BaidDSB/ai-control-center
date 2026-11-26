use super::help::guess_help_key;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
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

fn default_secondary_label() -> String {
    "Secondary Node".to_string()
}
fn default_rpc_port() -> u16 {
    50052
}
fn default_peer_port() -> u16 {
    8080
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum RpcFallbackPolicy {
    #[default]
    Auto,
    Strict,
}

impl RpcFallbackPolicy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (120ms fallback)",
            Self::Strict => "Strict (require node)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SecondaryNodeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_secondary_label")]
    pub label: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_rpc_port")]
    pub rpc_port: u16,
    #[serde(default = "default_peer_port")]
    pub peer_port: u16,
    #[serde(default)]
    pub rpc_fallback: RpcFallbackPolicy,
}

impl Default for SecondaryNodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            label: default_secondary_label(),
            host: String::new(),
            rpc_port: default_rpc_port(),
            peer_port: default_peer_port(),
            rpc_fallback: RpcFallbackPolicy::Auto,
        }
    }
}

impl SecondaryNodeConfig {
    pub fn rpc_endpoint(&self) -> Option<String> {
        let host = self.host.trim();
        if !self.enabled || host.is_empty() || self.rpc_port == 0 {
            return None;
        }
        if host.contains(':') && !host.starts_with('[') {
            Some(format!("[{host}]:{}", self.rpc_port))
        } else {
            Some(format!("{host}:{}", self.rpc_port))
        }
    }

    pub fn parse_host_port(endpoint: &str) -> Option<(String, u16)> {
        let s = endpoint.trim();
        if s.is_empty() {
            return None;
        }
        if let Some(rest) = s.strip_prefix('[') {
            let (h, p) = rest.split_once("]:")?;
            let port = p.trim().parse::<u16>().ok()?;
            if h.trim().is_empty() || port == 0 {
                return None;
            }
            return Some((h.trim().to_string(), port));
        }
        let (h, p) = s.rsplit_once(':')?;
        let port = p.trim().parse::<u16>().ok()?;
        if h.trim().is_empty() || port == 0 {
            return None;
        }
        Some((h.trim().to_string(), port))
    }
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
    #[serde(default)]
    pub secondary_node: SecondaryNodeConfig,
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

    pub fn migrate_legacy_rpc_to_secondary_node(&mut self) {
        if !self.secondary_node.host.trim().is_empty() {
            return;
        }
        let mut fallback_ep: Option<String> = None;
        let mut preferred_ep: Option<String> = None;
        for g in &self.groups {
            for p in &g.profiles {
                if !p.single.use_rpc {
                    continue;
                }
                let ep = p.single.rpc_endpoint.trim();
                if ep.is_empty() {
                    continue;
                }
                if !ep.starts_with("127.0.0.1:") && !ep.starts_with("localhost:") {
                    preferred_ep = Some(ep.to_string());
                    break;
                } else if fallback_ep.is_none() {
                    fallback_ep = Some(ep.to_string());
                }
            }
            if preferred_ep.is_some() {
                break;
            }
        }
        if let Some(ep) = preferred_ep.or(fallback_ep) {
            if let Some((host, port)) = SecondaryNodeConfig::parse_host_port(&ep) {
                self.secondary_node.enabled = true;
                self.secondary_node.host = host;
                self.secondary_node.rpc_port = port;
            }
        }
    }

    pub fn effective_rpc_for_profile(
        &self,
        spec: &ModelSpec,
        node_reachable: Option<bool>,
    ) -> Option<String> {
        if !spec.use_rpc {
            return None;
        }
        let ep = self.secondary_node.rpc_endpoint().or_else(|| {
            let legacy = spec.rpc_endpoint.trim();
            if !self.secondary_node.enabled
                && self.secondary_node.host.trim().is_empty()
                && !legacy.is_empty()
            {
                Some(legacy.to_string())
            } else {
                None
            }
        })?;
        if node_reachable == Some(false)
            && self.secondary_node.rpc_fallback == RpcFallbackPolicy::Auto
        {
            return None;
        }
        Some(ep)
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
                    cfg.migrate_legacy_rpc_to_secondary_node();
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
                migrated.migrate_legacy_rpc_to_secondary_node();
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
            secondary_node: SecondaryNodeConfig::default(),
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
