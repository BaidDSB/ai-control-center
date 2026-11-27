mod console;
mod drawer;
mod header;
mod inspector;
mod setup;
mod sidebar;
pub(crate) mod theme;
mod tray;

use crate::config::{
    parse_help_markdown, parse_help_markdown_str, scan_gguf_models, GgufInfo, HelpMap, StackConfig,
    ThemeMode,
};
use crate::{platform, runtime, AppPatch};
use console::{console_notifier, llama_vp_id, owu_vp_id, render_console_viewport};
use drawer::{render_right_drawer, CodePreviewScope, RightDrawerTab};
use eframe::egui;
use inspector::{render_advanced_inspector, render_spec_inspector};

use runtime::{
    active_backend_info, init_running_state, launch_routing_set,
    launch_single_profile, poll_tracked_processes, stop_tracked_backend,
    sync_live_llama_swap_yaml, ConsoleBuffer, ProcStatus, SharedConsole, SharedErrors,
};
use sidebar::{render_left_workspace, SidebarDrafts};
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use theme::{
    apply_app_theme, contrast_on, engine_badge_color_for, file_dialog_in, palette_for,
    pasteable_singleline, themed_card,
};
use tray::{TrayAction, TrayController};

fn load_window_icon(icon_path: &str, embedded: Option<&[u8]>) -> Option<egui::IconData> {
    let bytes = fs::read(icon_path.trim())
        .ok()
        .or_else(|| embedded.map(|b| b.to_vec()))?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

struct ControlCenterApp {
    patch: AppPatch,
    cfg: StackConfig,
    saved_cfg: StackConfig,
    scanned_models: Vec<String>,
    gguf_cache: HashMap<String, GgufInfo>,
    help_map: HelpMap,
    sidebar_drafts: SidebarDrafts,

    loaded_label: Option<String>,
    shared_status: Arc<Mutex<ProcStatus>>,
    shared_comp_ports: Arc<Mutex<Vec<u16>>>,
    shared_telemetry: Arc<Mutex<crate::runtime::TelemetrySnapshot>>,
    shared_api_target: Arc<Mutex<(String, u16)>>,
    llama_console: SharedConsole,
    owu_console: SharedConsole,
    show_llama_console: Arc<AtomicBool>,
    show_owu_console: Arc<AtomicBool>,
    error_logs: SharedErrors,

    show_error_log: bool,
    show_plain_hints: bool,
    show_theme_bar: bool,
    right_drawer: Option<RightDrawerTab>,
    code_scope: CodePreviewScope,
    live_code_select_mode: bool,
    copied_code_at: f64,
    edit_mode: bool,
    applied_theme_sig: (ThemeMode, [u8; 3], bool),
    status_msg: String,
    tray: TrayController,
    allow_close: bool,
}

impl ControlCenterApp {
    fn load_help_with_fallback(path: &str, embedded: Option<&'static str>) -> HelpMap {
        let resolved = StackConfig::resolve_config_relative(path);
        let from_disk = parse_help_markdown(&resolved);
        if !from_disk.is_empty() {
            from_disk
        } else if let Some(emb) = embedded {
            parse_help_markdown_str(emb)
        } else {
            from_disk
        }
    }

    fn new(ctx: egui::Context, patch: AppPatch) -> Self {
        let mut cfg = StackConfig::load_with_hooks(
            patch.default_config_fn,
            patch.legacy_migrator_fn,
            patch.post_load_fn,
        );
        if cfg.secondary_node.host.trim().is_empty()
            && let Some(ep) = patch.default_rpc_endpoint.as_deref()
                && let Some((host, port)) = crate::config::SecondaryNodeConfig::parse_host_port(ep) {
                    cfg.secondary_node.host = host;
                    cfg.secondary_node.rpc_port = port;
                }
        let saved_cfg = cfg.clone();
        let scanned_models = scan_gguf_models(&cfg.models_dir);
        let help_map = Self::load_help_with_fallback(&cfg.help_md_path, patch.embedded_help_md);
        let _ = init_running_state();
     let init_comp_ports: Vec<u16> = cfg
         .companion_services
         .iter()
         .map(|s| s.check_port)
         .collect();
     let initial = poll_tracked_processes(&init_comp_ports);
     let loaded_label = if initial.llama {
         active_backend_info()
             .map(|t| t.label)
             .filter(|s| !s.trim().is_empty())
             .or_else(|| Some(format!("Active Engine (:{})", cfg.server_port)))
     } else {
         None
     };

     let init_telemetry = crate::runtime::poll_telemetry(&cfg.server_host, cfg.server_port, initial.llama);
     let shared_status = Arc::new(Mutex::new(initial));
     let shared_comp_ports = Arc::new(Mutex::new(init_comp_ports));
     let shared_telemetry = Arc::new(Mutex::new(init_telemetry));
     let shared_api_target = Arc::new(Mutex::new((cfg.server_host.clone(), cfg.server_port)));
     let (bg_status, bg_ports, bg_tele, bg_api, bg_ctx) = (
         Arc::clone(&shared_status),
         Arc::clone(&shared_comp_ports),
         Arc::clone(&shared_telemetry),
         Arc::clone(&shared_api_target),
         ctx.clone(),
     );

     thread::spawn(move || loop {
         thread::sleep(Duration::from_millis(2500));
         let ports = bg_ports.lock().map(|g| g.clone()).unwrap_or_default();
         let latest = poll_tracked_processes(&ports);
         if let Ok(mut guard) = bg_status.lock() {
             *guard = latest;
         }
         let (api_host, api_port) = bg_api
             .lock()
             .map(|g| g.clone())
             .unwrap_or_else(|_| ("127.0.0.1".to_string(), 8080));
         let snap = crate::runtime::poll_telemetry(&api_host, api_port, latest.llama);
         if let Ok(mut guard) = bg_tele.lock() {
             *guard = snap;
         }
         bg_ctx.request_repaint_of(egui::ViewportId::ROOT);
     });

     let sig = (cfg.theme_mode, cfg.accent_rgb, false);
     let ready_msg = format!(
         "{} Ready — pick a Routing Set on top, or browse Model Families -> Sub-Profiles below.",
         patch.effective_version()
     );
     let tray_tip = format!(
         "{} {}",
         effective_app_title(&cfg, &patch),
         patch.effective_version()
     );
     let tray = TrayController::start(ctx.clone(), tray_tip);
     Self {
         patch,
         cfg,
         saved_cfg,
         scanned_models,
         gguf_cache: HashMap::new(),
         help_map,
         sidebar_drafts: SidebarDrafts::new(),
         loaded_label,
         shared_status,
         shared_comp_ports,
         shared_telemetry,
         shared_api_target,
         llama_console: Arc::new(Mutex::new(ConsoleBuffer::default())),
         owu_console: Arc::new(Mutex::new(ConsoleBuffer::default())),
         show_llama_console: Arc::new(AtomicBool::new(false)),
         show_owu_console: Arc::new(AtomicBool::new(false)),
         error_logs: Arc::new(Mutex::new(Vec::new())),
         show_error_log: false,
         show_plain_hints: true,
         show_theme_bar: false,
         right_drawer: None,
         code_scope: CodePreviewScope::RoutingSet,
         live_code_select_mode: false,
         copied_code_at: -10.0,
         edit_mode: false,
         applied_theme_sig: sig,
         status_msg: ready_msg,
         tray,
         allow_close: false,
     }
 }

 fn sync_backend_exes(&self) {
     if let Ok(mut g) = self.shared_comp_ports.lock() {
         *g = self
             .cfg
             .companion_services
             .iter()
             .map(|s| s.check_port)
             .collect();
     }
     if let Ok(mut g) = self.shared_api_target.lock() {
         *g = (self.cfg.server_host.clone(), self.cfg.server_port);
     }
 }

 fn reload_help(&mut self) {
     self.help_map =
         Self::load_help_with_fallback(&self.cfg.help_md_path, self.patch.embedded_help_md);
     self.status_msg = format!("Reloaded help notes from {}", self.cfg.help_md_path);
 }

 fn push_error(&mut self, err: String) {
     let num = if let Ok(mut logs) = self.error_logs.lock() {
         let n = logs.len() + 1;
         logs.push(format!("[ERR #{n}] {err}"));
         n
     } else {
         1
     };
     self.show_error_log = true;
     self.status_msg = format!("Error #{num} logged — open Error Log below.");
 }

 fn save_now(&mut self, note: &str) {
     self.sync_backend_exes();
     match self.cfg.save() {
         Ok(()) => {
             self.saved_cfg = self.cfg.clone();
             match sync_live_llama_swap_yaml(&self.cfg) {
                 Ok(true) => {
                     self.status_msg = format!("{note} (Synced live llama-swap.yaml)");
                 }
                 Ok(false) => {
                     self.status_msg = note.to_string();
                 }
                 Err(e) => {
                     self.status_msg = format!("{note} (Live YAML sync skipped: {e})");
                 }
             }
         }
         Err(e) => self.push_error(format!("Config save failed: {e}")),
     }
 }

    fn current_procs(&self) -> ProcStatus {
        self.shared_status.lock().map(|g| *g).unwrap_or_default()
    }

    fn set_procs(&self, s: ProcStatus) {
        if let Ok(mut g) = self.shared_status.lock() {
            *g = s;
        }
    }

    pub(super) fn current_telemetry(&self) -> crate::runtime::TelemetrySnapshot {
        self.shared_telemetry
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

impl eframe::App for ControlCenterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let want_sig = (self.cfg.theme_mode, self.cfg.accent_rgb, self.edit_mode);
        if want_sig != self.applied_theme_sig {
            apply_app_theme(
                ui.ctx(),
                self.cfg.theme_mode,
                self.cfg.accent_rgb,
                self.edit_mode,
            );
            self.applied_theme_sig = want_sig;
        }

        let pal = palette_for(self.cfg.theme_mode, self.cfg.accent_rgb, self.edit_mode);
        let is_light = self.cfg.theme_mode == ThemeMode::Light;

        // SEPARATE NATIVE CONSOLE VIEWPORTS
        render_console_viewport(
            ui.ctx(),
            llama_vp_id(),
            "Engine / llama-swap — Live Console",
            pal.accent,
            Arc::clone(&self.show_llama_console),
            Arc::clone(&self.llama_console),
            Arc::clone(&self.shared_status),
            true,
        );
        render_console_viewport(
            ui.ctx(),
            owu_vp_id(),
            "Open WebUI — Live Console",
            egui::Color32::from_rgb(52, 211, 153),
            Arc::clone(&self.show_owu_console),
            Arc::clone(&self.owu_console),
            Arc::clone(&self.shared_status),
            false,
        );

        let mut procs = self.current_procs();
        if !procs.llama {
            self.loaded_label = None;
        }

        // SYSTEM TRAY ACTIONS & CLOSE-TO-TRAY INTERCEPT
        for action in self.tray.poll_actions() {
            match action {
                TrayAction::Show => {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayAction::OpenBrowser => {
                    platform::open_url(&self.cfg.effective_frontend_url());
                }
                TrayAction::SoftUnload => {
                    match runtime::soft_unload_llama_swap(&self.cfg.server_host, self.cfg.server_port) {
                        Ok(()) => {
                            if let Ok(mut g) = self.shared_telemetry.lock() {
                                g.running_models.clear();
                            }
                            self.status_msg = "Sent Soft Unload via tray — VRAM freeing.".to_string();
                        }
                        Err(e) => self.push_error(format!("Tray Soft Unload failed: {e}")),
                    }
                }
                TrayAction::StopAll => {
                    stop_tracked_backend();
                    runtime::stop_tracked_frontend();
                    runtime::stop_tracked_mcpo();
                    if cfg!(feature = "plus") {
                        for (ci, svc) in self.cfg.companion_services.iter().enumerate() {
                            if svc.enabled
                                && ci < procs.companion_running.len()
                                && procs.companion_running[ci]
                            {
                                let _ = runtime::stop_companion_service(svc);
                            }
                        }
                    }
                    self.loaded_label = None;
                    procs = ProcStatus::default();
                    self.set_procs(procs);
                    self.status_msg = "Stopped all services via tray.".to_string();
                }
                TrayAction::Quit => {
                    self.allow_close = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.cfg.minimize_to_tray
            && !self.allow_close
            && self.tray.is_active()
            && ui.ctx().input(|i| i.viewport().close_requested())
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        let err_count = self.error_logs.lock().map(|l| l.len()).unwrap_or(0);
        let edit_mode = self.edit_mode;

        // RIGHT-SIDE TABBED DRAWER (HELP | CODE / YAML)
        let drawer_out = render_right_drawer(
            ui,
            pal,
            &self.cfg,
            &self.saved_cfg,
            &self.help_map,
            &mut self.right_drawer,
            &mut self.code_scope,
            &mut self.live_code_select_mode,
            &mut self.copied_code_at,
        );
        if drawer_out.reload_help {
            self.reload_help();
        }
        if let Some(msg) = drawer_out.status_msg {
            self.status_msg = msg;
        }

        // MAIN CENTRAL PANEL
        egui::CentralPanel::default().show(ui, |ui| {
            // TOP HEADER CARD
            procs = self.render_top_header(ui, pal, procs, err_count, edit_mode);

            ui.add_space(8.0);
            let log_reserve = if self.show_error_log { 155.0 } else { 0.0 };
            let avail_height = (ui.available_height() - log_reserve).max(280.0);

            // MAIN SPLIT: LEFT SELECTION WORKSPACE | RIGHT FULL-HEIGHT INSPECTOR
            let total_w = ui.available_width();
            let col_gap = 8.0;
            let left_workspace_w = ((total_w - col_gap) * 0.5).floor().max(340.0);
            let right_width = (total_w - col_gap - left_workspace_w).max(340.0);
            let col1_w = ((left_workspace_w - col_gap) * 0.48).floor().max(160.0);
            let col2_w = (left_workspace_w - col_gap - col1_w).max(160.0);

            ui.horizontal_top(|ui| {
                let side_out = render_left_workspace(
                    ui,
                    left_workspace_w,
                    avail_height,
                    col_gap,
                    col1_w,
                    col2_w,
                    edit_mode,
                    pal,
                    &mut self.cfg,
                    &mut self.saved_cfg,
                    &mut self.sidebar_drafts,
                    &mut self.scanned_models,
                    &mut self.gguf_cache,
                    &mut self.status_msg,
                );
                if side_out.reload_help {
                    self.help_map = Self::load_help_with_fallback(
                        &self.cfg.help_md_path,
                        self.patch.embedded_help_md,
                    );
                }
                if side_out.sync_exes {
                    self.sync_backend_exes();
                }
                if let Some(note) = side_out.save_note {
                    self.save_now(&note);
                }

                ui.add_space(col_gap);

                // RIGHT WORKSPACE: FULL-HEIGHT PROFILE INSPECTOR
                let saved_engines_snap = self.cfg.saved_engines.clone();
                let global_llama_dir = self.cfg.llama_dir.clone();
                let saved_engines_for_colors = self.cfg.saved_engines.clone();

                let loaded_col = if is_light {
                    egui::Color32::from_rgb(14, 124, 78)
                } else {
                    egui::Color32::from_rgb(52, 211, 153)
                };
                let unloaded_col = if is_light {
                    egui::Color32::from_rgb(168, 98, 8)
                } else {
                    egui::Color32::from_rgb(251, 191, 36)
                };

                ui.allocate_ui_with_layout(
                    egui::vec2(right_width, avail_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(right_width);
                        ui.set_max_width(right_width);
                        themed_card(pal).show(ui, |ui| {
                            let r_in = (right_width - 26.0).max(240.0);
                            ui.set_min_width(r_in);
                            ui.set_max_width(r_in);
                            let g_idx = self.cfg.selected_group_idx;
                            let p_idx = self.cfg.selected_profile_idx;
                            let models_dir = self.cfg.models_dir.clone();
                            let mut trigger_save_msg: Option<String> = None;
                            let mut revert_requested = false;
                            let has_unsaved = self.cfg != self.saved_cfg;

                            egui::ScrollArea::vertical()
                                .id_salt("right_inspector_scroll")
                                .max_height((avail_height - 24.0).max(160.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let (vram_txt, vram_col) = if procs.llama {
                                        let name =
                                            self.loaded_label.as_deref().unwrap_or("Active Engine");
                                        (format!("[LOADED IN VRAM] {name}"), loaded_col)
                                    } else {
                                        (
                                            "[UNLOADED] 0 MB VRAM Used (Safe for Gaming)"
                                                .to_string(),
                                            unloaded_col,
                                        )
                                    };
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(vram_txt).strong().color(vram_col),
                                        )
                                        .selectable(false),
                                    );
                                    ui.add_space(4.0);

                                    // PRIMARY ACTION BUTTONS
                                    ui.horizontal_wrapped(|ui| {
                                        let active_set_name = self
                                            .cfg
                                            .routing_sets
                                            .get(self.cfg.selected_set_idx)
                                            .map(|s| s.name.clone())
                                            .unwrap_or_else(|| "Set".to_string());

                                        let load_set_btn = egui::Button::new(
                                            egui::RichText::new(format!(
                                                "> LAUNCH SET: {active_set_name}"
                                            ))
                                            .strong()
                                            .color(pal.accent_text),
                                        )
                                        .fill(pal.accent);
                                        if ui.add_sized([210.0, 32.0], load_set_btn).clicked() {
                                            self.sync_backend_exes();
                                            if let Some(set) = self
                                                .cfg
                                                .routing_sets
                                                .get(self.cfg.selected_set_idx)
                                                .cloned()
                                            {
                                                match launch_routing_set(
                                                    &self.cfg,
                                                    &set,
                                                    Arc::clone(&self.llama_console),
                                                    Arc::clone(&self.error_logs),
                                                    console_notifier(
                                                        ui.ctx().clone(),
                                                        llama_vp_id(),
                                                    ),
                                                ) {
                                                    Ok(()) => {
                                                        self.loaded_label = Some(format!(
                                                            "Routing Set: {} [{}]",
                                                            set.name, set.mode
                                                        ));
                                                        procs.llama = true;
                                                        self.set_procs(procs);
                                                        self.status_msg = format!(
                                                            "Launched Routing Set '{}' ({} linked models).",
                                                            set.name,
                                                            set.profile_ids.len()
                                                        );
                                                    }
                                                    Err(e) => self.push_error(e),
                                                }
                                            }
                                        }

                                        let direct_btn = egui::Button::new(
                                            egui::RichText::new("> Load Single Profile")
                                                .strong()
                                                .color(pal.btn_idle_text),
                                        )
                                        .fill(pal.btn_idle);
                                        if ui.add_sized([150.0, 32.0], direct_btn).clicked() {
                                            self.sync_backend_exes();
                                            let target = self
                                                .cfg
                                                .groups
                                                .get(self.cfg.selected_group_idx)
                                                .and_then(|g| {
                                                    g.profiles
                                                        .get(self.cfg.selected_profile_idx)
                                                        .map(|p| (g.model_title.clone(), p.clone()))
                                                });
                                            if let Some((gtitle, prof)) = target {
                                                match launch_single_profile(
                                                    &self.cfg,
                                                    &prof,
                                                    Arc::clone(&self.llama_console),
                                                    Arc::clone(&self.error_logs),
                                                    console_notifier(
                                                        ui.ctx().clone(),
                                                        llama_vp_id(),
                                                    ),
                                                ) {
                                                    Ok(()) => {
                                                        self.loaded_label = Some(format!(
                                                            "{gtitle} -> {} [{}]",
                                                            prof.name,
                                                            prof.engine_badge_name()
                                                        ));
                                                        procs.llama = true;
                                                        self.set_procs(procs);
                                                        self.status_msg = format!(
                                                            "Loaded '{gtitle}' ({}) via {}.",
                                                            prof.name,
                                                            prof.effective_engine_exe()
                                                        );
                                                    }
                                                    Err(e) => self.push_error(e),
                                                }
                                            }
                                        }

                                        let unload_btn = egui::Button::new(
                                            egui::RichText::new("X UNLOAD")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(egui::Color32::from_rgb(153, 27, 27));
                                        if ui.add_sized([100.0, 32.0], unload_btn).clicked() {
                                            stop_tracked_backend();
                                            self.loaded_label = None;
                                            procs.llama = false;
                                            self.set_procs(procs);
                                            self.status_msg =
                                                "Engine unloaded. VRAM freed!".to_string();
                                        }

                                        let save_fill = if edit_mode {
                                            pal.edit_hl
                                        } else {
                                            egui::Color32::from_rgb(16, 125, 78)
                                        };
                                        let save_btn = egui::Button::new(
                                            egui::RichText::new("Save Changes")
                                                .strong()
                                                .color(contrast_on(save_fill)),
                                        )
                                        .fill(save_fill);
                                        if ui.add_sized([112.0, 32.0], save_btn).clicked() {
                                            trigger_save_msg = Some(
                                                "Saved all edits, routing sets, and flags."
                                                    .to_string(),
                                            );
                                        }

                                        if ui
                                            .add_enabled(
                                                has_unsaved,
                                                egui::Button::new("Revert")
                                                    .min_size(egui::vec2(68.0, 32.0)),
                                            )
                                            .clicked()
                                        {
                                            revert_requested = true;
                                        }
                                        if ui
                                            .add(egui::Button::new("Export").min_size(egui::vec2(64.0, 32.0)))
                                            .on_hover_text("Export this sub-profile as a path-stripped portable .toml file")
                                            .clicked()
                                            && let Some(prof) = self
                                                .cfg
                                                .groups
                                                .get(g_idx)
                                                .and_then(|g| g.profiles.get(p_idx))
                                        {
                                            let default_name = format!("{}.toml", crate::config::slugify(&prof.name));
                                            if let Some(save_path) = file_dialog_in("")
                                                .set_file_name(&default_name)
                                                .add_filter("Profile TOML", &["toml"])
                                                .save_file()
                                            {
                                                match prof.to_portable_toml() {
                                                    Ok(toml_body) => match fs::write(&save_path, toml_body) {
                                                        Ok(()) => {
                                                            self.status_msg = format!(
                                                                "Exported portable profile to {}",
                                                                save_path.display()
                                                            );
                                                        }
                                                        Err(e) => self.push_error(format!("Export write failed: {e}")),
                                                    },
                                                    Err(e) => self.push_error(format!("Export serialize failed: {e}")),
                                                }
                                            }
                                        }
                                        if has_unsaved {
                                            ui.label(
                                                egui::RichText::new("Unsaved / Temp")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(unloaded_col),
                                            );
                                        }
                                    });

                                    ui.add_space(6.0);
                                    ui.separator();

                                    if let Some(grp) = self.cfg.groups.get_mut(g_idx) {
                                        if edit_mode {
                                            egui::Frame::new()
                                                .fill(pal.item_bg)
                                                .stroke(egui::Stroke::new(1.0, pal.edit_hl))
                                                .corner_radius(egui::CornerRadius::same(10))
                                                .inner_margin(egui::Margin::same(10))
                                                .show(ui, |ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new("EDIT MODEL FAMILY, SUB-PROFILE, HELP KEY & BOUND ENGINE")
                                                                .size(11.5)
                                                                .strong()
                                                                .color(pal.edit_hl),
                                                        )
                                                        .selectable(false),
                                                    );

                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.label("Family Name:");
                                                        pasteable_singleline(ui, &mut grp.model_title, 200.0, "");
                                                        ui.label("Tagline:");
                                                        pasteable_singleline(ui, &mut grp.subtitle, 200.0, "");
                                                    });

                                                    if let Some(prof) = grp.profiles.get_mut(p_idx) {
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label("Sub-Profile:");
                                                            pasteable_singleline(ui, &mut prof.name, 215.0, "");
                                                            ui.label("Role Badge:");
                                                            pasteable_singleline(ui, &mut prof.role_tag, 185.0, "");
                                                        });
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label("Help Key (@model):");
                                                            pasteable_singleline(ui, &mut prof.help_key, 135.0, "e.g. qwen3.6-35b");
                                                            ui.label("Variant:");
                                                            pasteable_singleline(ui, &mut prof.help_variant, 95.0, "optional");
                                                            ui.label("Idle TTL (s):");
                                                            ui.add(egui::DragValue::new(&mut prof.ttl_seconds).range(0..=7200).speed(30))
                                                                .on_hover_text("Auto-unload seconds in llama-swap (0 = never unload).");
                                                            ui.label("Health:");
                                                            pasteable_singleline(ui, &mut prof.check_endpoint, 95.0, "/health or none")
                                                                .on_hover_text("Optional llama-swap checkEndpoint (leave blank for /health, or set 'none' / '/v1/models').");
                                                        });
                                                        ui.horizontal_wrapped(|ui| {
                                                            egui::ComboBox::from_id_salt("bind_saved_engine")
                                                                .selected_text("Bind Saved Engine...")
                                                                .width(175.0)
                                                                .show_ui(ui, |ui| {
                                                                    if ui
                                                                        .selectable_label(
                                                                            prof.engine_dir_override.is_empty() && prof.engine_label.is_empty(),
                                                                            "Use Global Default",
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        prof.engine_label.clear();
                                                                        prof.engine_kind.clear();
                                                                        prof.engine_dir_override.clear();
                                                                        prof.engine_exe_override.clear();
                                                                        trigger_save_msg = Some("Reset sub-profile to Global Default engine.".to_string());
                                                                    }
                                                                    for eng in &saved_engines_snap {
                                                                        let lbl = format!("{} ({})", eng.label, eng.exe_name);
                                                                        if ui.selectable_label(false, lbl).clicked() {
                                                                            prof.engine_label = eng.label.clone();
                                                                            prof.engine_kind = eng.engine_kind.clone();
                                                                            prof.engine_dir_override = eng.dir_path.clone();
                                                                            prof.engine_exe_override = eng.exe_name.clone();
                                                                            if eng.engine_kind == "custom"
                                                                                && prof.single.extra_args.trim().is_empty()
                                                                                && !eng.default_args.trim().is_empty()
                                                                            {
                                                                                prof.single.extra_args = eng.default_args.clone();
                                                                            }
                                                                            trigger_save_msg = Some(format!("Bound sub-profile to engine '{}'.", eng.label));
                                                                        }
                                                                    }
                                                                });

                                                            ui.label("Engine Label:");
                                                            pasteable_singleline(ui, &mut prof.engine_label, 130.0, "Global Default");

                                                            let cur_mode = if prof.effective_engine_kind() == "custom" {
                                                                "custom"
                                                            } else {
                                                                "llama_cpp"
                                                            };
                                                            ui.label("CLI Style:");
                                                            egui::ComboBox::from_id_salt("prof_eng_kind")
                                                                .selected_text(cur_mode)
                                                                .width(88.0)
                                                                .show_ui(ui, |ui| {
                                                                    ui.selectable_value(&mut prof.engine_kind, "llama_cpp".to_string(), "llama_cpp");
                                                                    ui.selectable_value(&mut prof.engine_kind, "custom".to_string(), "custom");
                                                                });
                                                        });
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label("Exe:");
                                                            pasteable_singleline(ui, &mut prof.engine_exe_override, 165.0, &platform::default_exe_name("llama-server"));
                                                            if ui.button("Pick .exe").clicked() {
                                                                let start_dir = if prof.engine_dir_override.trim().is_empty() {
                                                                    global_llama_dir.as_str()
                                                                } else {
                                                                    prof.engine_dir_override.as_str()
                                                                };
                                                                let mut dlg = file_dialog_in(start_dir);
                                                                if cfg!(windows) {
                                                                    dlg = dlg.add_filter("Executable", &["exe"]);
                                                                }
                                                                if let Some(p) = dlg.pick_file() {
                                                                    if let Some(par) = p.parent() {
                                                                        prof.engine_dir_override = par.to_string_lossy().to_string();
                                                                    }
                                                                    if let Some(fname) = p.file_name() {
                                                                        prof.engine_exe_override = fname.to_string_lossy().to_string();
                                                                    }
                                                                    if prof.engine_label.trim().is_empty()
                                                                        && let Some(stem) = p.file_stem() {
                                                                            prof.engine_label = stem.to_string_lossy().to_string();
                                                                        }
                                                                    trigger_save_msg = Some("Bound custom engine executable to sub-profile.".to_string());
                                                                }
                                                            }
                                                            ui.label("Folder:");
                                                            pasteable_singleline(ui, &mut prof.engine_dir_override, 210.0, &global_llama_dir);
                                                        });

                                                        ui.label("Description:");
                                                        ui.add(
                                                            egui::TextEdit::multiline(&mut prof.packed_description)
                                                                .desired_rows(2)
                                                                .desired_width((right_width - 56.0).max(240.0)),
                                                        );
                                                    }
                                                });
                                            ui.add_space(6.0);
                                        } else if let Some(prof) = grp.profiles.get(p_idx) {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(format!(
                                                        "{} — {}",
                                                        grp.model_title, prof.name
                                                    ))
                                                    .size(15.0)
                                                    .strong()
                                                    .color(pal.text),
                                                )
                                                .selectable(false),
                                            );
                                            let bound_dir =
                                                prof.effective_engine_dir(&global_llama_dir);
                                            let ecol = engine_badge_color_for(
                                                prof.engine_badge_name(),
                                                &saved_engines_for_colors,
                                            );
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(format!(
                                                        "Engine [{}]: {} -> {}",
                                                        prof.engine_badge_name(),
                                                        prof.effective_engine_exe(),
                                                        bound_dir
                                                    ))
                                                    .size(11.0)
                                                    .color(ecol),
                                                )
                                                .selectable(false),
                                            );
                                            ui.add_space(4.0);
                                            egui::Frame::new()
                                                .fill(pal.item_bg)
                                                .stroke(egui::Stroke::new(1.0, pal.item_border))
                                                .corner_radius(egui::CornerRadius::same(10))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(
                                                            &prof.packed_description,
                                                        )
                                                        .size(12.0)
                                                        .color(pal.text),
                                                    );
                                                });
                                            ui.add_space(6.0);
                                        }

                                        if let Some(prof) = grp.profiles.get_mut(p_idx) {
                                            ui.horizontal(|ui| {
                                                let simple_sel = !prof.use_manual_flags;
                                                let adv_sel = prof.use_manual_flags;

                                                let s_btn = egui::Button::new(
                                                    egui::RichText::new("Simple Mode")
                                                        .strong()
                                                        .color(if simple_sel {
                                                            pal.accent_text
                                                        } else {
                                                            pal.btn_idle_text
                                                        }),
                                                )
                                                .fill(if simple_sel {
                                                    pal.accent
                                                } else {
                                                    pal.btn_idle
                                                });
                                                if ui.add(s_btn).clicked() {
                                                    prof.use_manual_flags = false;
                                                }

                                                let a_btn = egui::Button::new(
                                                    egui::RichText::new("Advanced (Manual Flags)")
                                                        .strong()
                                                        .color(if adv_sel {
                                                            pal.accent_text
                                                        } else {
                                                            pal.btn_idle_text
                                                        }),
                                                )
                                                .fill(if adv_sel {
                                                    pal.accent
                                                } else {
                                                    pal.btn_idle
                                                });
                                                if ui.add(a_btn).clicked() {
                                                    if prof.manual_flags.trim().is_empty() {
                                                        prof.manual_flags =
                                                            prof.generate_manual_template();
                                                    }
                                                    prof.use_manual_flags = true;
                                                }

                                                if prof.use_manual_flags {
                                                    ui.label(
                                                        egui::RichText::new("Using manual flags")
                                                            .size(11.0)
                                                            .color(pal.muted),
                                                    );
                                                }
                                            });
                                            ui.add_space(4.0);

                                            if prof.use_manual_flags {
                                                render_advanced_inspector(
                                                    ui,
                                                    prof,
                                                    edit_mode,
                                                    self.show_plain_hints,
                                                    is_light,
                                                    pal,
                                                    &models_dir,
                                                    &mut self.scanned_models,
                                                    &mut self.gguf_cache,
                                                    &mut trigger_save_msg,
                                                );
                                            } else {
                                                let sum_col = if edit_mode {
                                                    pal.edit_hl
                                                } else {
                                                    pal.accent
                                                };
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(format!(
                                                            "Active Flags: {}",
                                                            prof.single.short_summary()
                                                        ))
                                                        .size(11.5)
                                                        .color(sum_col),
                                                    )
                                                    .selectable(false),
                                                );

                                                let sec_rpc_ep = self
                                                    .cfg
                                                    .secondary_node
                                                    .rpc_endpoint()
                                                    .or_else(|| {
                                                        let legacy = prof.single.rpc_endpoint.trim();
                                                        if !self.cfg.secondary_node.enabled
                                                            && self.cfg.secondary_node.host.trim().is_empty()
                                                            && !legacy.is_empty()
                                                        {
                                                            Some(legacy.to_string())
                                                        } else {
                                                            None
                                                        }
                                                    });
                                                let rpc_fallback = self.cfg.secondary_node.rpc_fallback;
                                                let local_vram_mib = self
                                                    .shared_telemetry
                                                    .lock()
                                                    .ok()
                                                    .and_then(|t| t.gpu_vram.as_ref().map(|g| g.total_mib))
                                                    .filter(|&m| m > 0);
                                                let mut port_copy = prof.port;
                                                render_spec_inspector(
                                                    ui,
                                                    &mut prof.single,
                                                    edit_mode,
                                                    self.show_plain_hints,
                                                    is_light,
                                                    pal,
                                                    &models_dir,
                                                    &mut self.scanned_models,
                                                    &mut self.gguf_cache,
                                                    &mut port_copy,
                                                    sec_rpc_ep.as_deref(),
                                                    rpc_fallback,
                                                    local_vram_mib,
                                                    "cc_single_slot",
                                                    &mut trigger_save_msg,
                                                );
                                                prof.port = port_copy;
                                            }
                                        }
                                    }
                                });

                            if revert_requested {
                                self.cfg = self.saved_cfg.clone();
                                self.sync_backend_exes();
                                self.status_msg = "Reverted all unsaved changes.".to_string();
                            } else if let Some(msg) = trigger_save_msg {
                                self.save_now(&msg);
                            }
                        });
                    },
                );
            });

            // SEPARATE COPYABLE ERROR LOG PANEL
            if self.show_error_log {
                let err_hdr_col = if is_light {
                    egui::Color32::from_rgb(185, 28, 28)
                } else {
                    egui::Color32::from_rgb(248, 113, 113)
                };
                ui.add_space(8.0);
                themed_card(pal).show(ui, |ui| {
                    let full_log = self
                        .error_logs
                        .lock()
                        .map(|l| {
                            if l.is_empty() {
                                "No errors logged yet.".to_string()
                            } else {
                                l.join("\n\n")
                            }
                        })
                        .unwrap_or_default();

                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("ERROR & DIAGNOSTICS LOG (SELECTABLE)")
                                    .strong()
                                    .color(err_hdr_col),
                            )
                            .selectable(false),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                self.show_error_log = false;
                            }
                            if ui.button("Clear Log").clicked()
                                && let Ok(mut l) = self.error_logs.lock() {
                                    l.clear();
                                }
                            if ui.button("Notepad").clicked() {
                                runtime::open_text_in_notepad("error_log.txt", &full_log);
                            }
                            let copy_btn = egui::Button::new(
                                egui::RichText::new("Copy Error Log")
                                    .strong()
                                    .color(pal.accent_text),
                            )
                            .fill(pal.accent);
                            if ui.add(copy_btn).clicked() && runtime::copy_to_clipboard(&full_log) {
                                self.status_msg = "Copied error log to clipboard!".to_string();
                            }
                        });
                    });

                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("cc_err_scroll")
                        .max_height(88.0)
                        .show(ui, |ui| {
                            let mut slice: &str = full_log.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut slice)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_rows(4)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                });
            }
        });
    }
}

fn effective_app_title<'a>(cfg: &'a StackConfig, patch: &'a AppPatch) -> &'a str {
    let custom = cfg.app_title.trim();
    if custom.is_empty() || custom == "AI Control Center" {
        patch.app_title.as_deref().unwrap_or("AI Control Center")
    } else {
        custom
    }
}

pub fn run_app(patch: AppPatch) -> eframe::Result<()> {
    let cfg = StackConfig::load_with_hooks(
        patch.default_config_fn,
        patch.legacy_migrator_fn,
        patch.post_load_fn,
    );
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1440.0, 860.0])
        .with_min_inner_size([1120.0, 660.0]);
    if let Some(icon) = load_window_icon(&cfg.favicon_path, patch.embedded_icon_bytes) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let (init_mode, init_accent) = (cfg.theme_mode, cfg.accent_rgb);
    let title = format!(
        "{} {}",
        effective_app_title(&cfg, &patch),
        patch.effective_version()
    );
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            apply_app_theme(&cc.egui_ctx, init_mode, init_accent, false);
            Ok(Box::new(ControlCenterApp::new(cc.egui_ctx.clone(), patch)))
        }),
    )
}
