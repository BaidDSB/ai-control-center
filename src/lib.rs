pub mod app;
pub mod config;
pub mod platform;
pub mod runtime;

use config::StackConfig;

/// Overlay hook allowing a private crate (Tier 3: Personal) to inject defaults,
/// branding, hardware hints, embedded fallback help.md, and custom icon bytes
/// without adding any personal data to the public Core / Plus repository.
#[derive(Clone, Default)]
pub struct AppPatch {
    pub app_title: Option<String>,
    pub version_label: Option<String>,
    pub hardware_hint_label: Option<String>,
    pub default_rpc_endpoint: Option<String>,
    pub default_config_fn: Option<fn() -> StackConfig>,
    pub post_load_fn: Option<fn(&mut StackConfig)>,
    pub legacy_migrator_fn: Option<fn(&str) -> Result<StackConfig, String>>,
    pub embedded_help_md: Option<&'static str>,
    pub embedded_icon_bytes: Option<&'static [u8]>,
}

impl AppPatch {
    pub fn tier_name(&self) -> &'static str {
        if self.default_config_fn.is_some() {
            "Personal"
        } else if cfg!(feature = "plus") {
            "Plus"
        } else {
            "Core"
        }
    }

    pub fn effective_version(&self) -> &str {
        self.version_label.as_deref().unwrap_or("v1.0")
    }

    pub fn effective_hardware_label(&self) -> &str {
        self.hardware_hint_label
            .as_deref()
            .unwrap_or("GPU VRAM + System RAM")
    }

    pub fn effective_rpc_default(&self) -> &str {
        self.default_rpc_endpoint
            .as_deref()
            .unwrap_or("127.0.0.1:50052")
    }
}
pub use app::run_app;
