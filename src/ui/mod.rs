mod console;
mod drawer;
mod inspector;
mod sidebar;
mod theme;

use crate::config::{
    parse_help_markdown, parse_help_markdown_str, scan_gguf_models, GgufInfo, HelpMap, StackConfig,
    ThemeMode,
};
use crate::{platform, runtime, AppPatch};
use console::{console_notifier, llama_vp_id, owu_vp_id, render_console_viewport};
use drawer::{render_right_drawer, CodePreviewScope, RightDrawerTab};
use eframe::egui;
use inspector::{render_advanced_inspector, render_spec_inspector};
use rfd::FileDialog;
use runtime::{
    collect_backend_exes, launch_mcpo, launch_open_webui, launch_routing_set,
    launch_single_profile, poll_processes_full, run_bat_file, start_companion_service,
    stop_all_backends, stop_companion_service, stop_process, ConsoleBuffer, ProcStatus,
    SharedConsole, SharedErrors,
};
use sidebar::{render_left_workspace, SidebarDrafts};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use theme::{
    apply_app_theme, contrast_on, engine_badge_color_for, palette_for, pasteable_singleline,
    themed_card,
};

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
    shared_exes: Arc<Mutex<Vec<String>>>,
    shared_fe_exe: Arc<Mutex<String>>,
    shared_comp_ports: Arc<Mutex<Vec<u16>>>,
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
        let cfg = StackConfig::load_with_hooks(
            patch.default_config_fn,
            patch.legacy_migrator_fn,
            patch.post_load_fn,
        );
        let saved_cfg = cfg.clone();
        let scanned_models = scan_gguf_models(&cfg.models_dir);
        let help_map = Self::load_help_with_fallback(&cfg.help_md_path, patch.embedded_help_md);
        let init_exes = collect_backend_exes(&cfg);
        let init_fe_exe = cfg.effective_frontend_exe_name();
        let init_comp_ports: Vec<u16> = cfg
            .companion_services
            .iter()
            .map(|s| s.check_port)
            .collect();
        let initial = poll_processes_full(&init_exes, &init_fe_exe, &init_comp_ports);
        let loaded_label = if initial.llama {
            Some(format!("Active Engine (:{})", cfg.server_port))
        } else {
            None
        };

        let shared_status = Arc::new(Mutex::new(initial));
        let shared_exes = Arc::new(Mutex::new(init_exes));
        let shared_fe_exe = Arc::new(Mutex::new(init_fe_exe));
        let shared_comp_ports = Arc::new(Mutex::new(init_comp_ports));
        let (bg_status, bg_exes, bg_fe, bg_ports, bg_ctx) = (
            Arc::clone(&shared_status),
            Arc::clone(&shared_exes),
            Arc::clone(&shared_fe_exe),
            Arc::clone(&shared_comp_ports),
            ctx.clone(),
        );

        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(2500));
            let exes = bg_exes.lock().map(|g| g.clone()).unwrap_or_else(|_| {
                vec![
                    platform::default_exe_name("llama-swap"),
                    platform::default_exe_name("llama-server"),
                ]
            });
            let fe = bg_fe.lock().map(|g| g.clone()).unwrap_or_default();
            let ports = bg_ports.lock().map(|g| g.clone()).unwrap_or_default();
            let latest = poll_processes_full(&exes, &fe, &ports);
            if let Ok(mut guard) = bg_status.lock() {
                *guard = latest;
            }
            bg_ctx.request_repaint_of(egui::ViewportId::ROOT);
        });

        let sig = (cfg.theme_mode, cfg.accent_rgb, false);
        let ready_msg = format!(
            "{} Ready — pick a Routing Set on top, or browse Model Families -> Sub-Profiles below.",
            patch.effective_version()
        );
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
            shared_exes,
            shared_fe_exe,
            shared_comp_ports,
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
        }
    }

    fn sync_backend_exes(&self) {
        if let Ok(mut g) = self.shared_exes.lock() {
            *g = collect_backend_exes(&self.cfg);
        }
        if let Ok(mut g) = self.shared_fe_exe.lock() {
            *g = self.cfg.effective_frontend_exe_name();
        }
        if let Ok(mut g) = self.shared_comp_ports.lock() {
            *g = self
                .cfg
                .companion_services
                .iter()
                .map(|s| s.check_port)
                .collect();
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
                self.status_msg = note.to_string();
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
            themed_card(pal).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let title_str = if edit_mode {
                        format!(
                            "{} {}  [EDIT MODE]",
                            effective_app_title(&self.cfg, &self.patch).to_uppercase(),
                            self.patch.effective_version()
                        )
                    } else {
                        format!(
                            "{} {}",
                            effective_app_title(&self.cfg, &self.patch).to_uppercase(),
                            self.patch.effective_version()
                        )
                    };
                    let title_col = if edit_mode { pal.edit_hl } else { pal.text };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(title_str)
                                .size(17.5)
                                .strong()
                                .color(title_col),
                        )
                        .selectable(false),
                    );
                    ui.add_space(8.0);
                    let st_col = if edit_mode { pal.edit_hl } else { pal.accent };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.status_msg)
                                .size(12.0)
                                .color(st_col),
                        )
                        .selectable(false),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let err_lbl = format!("Error Log ({err_count})");
                        let err_fill = if err_count > 0 {
                            egui::Color32::from_rgb(165, 32, 38)
                        } else {
                            pal.btn_idle
                        };
                        let err_txt = if err_count > 0 {
                            egui::Color32::WHITE
                        } else {
                            pal.btn_idle_text
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(err_lbl).strong().color(err_txt),
                                )
                                .fill(err_fill),
                            )
                            .clicked()
                        {
                            self.show_error_log = !self.show_error_log;
                        }

                        let code_open = self.right_drawer == Some(RightDrawerTab::CodeYaml);
                        let code_btn = egui::Button::new(
                            egui::RichText::new("Code / YAML").strong().color(if code_open {
                                pal.accent_text
                            } else {
                                pal.btn_idle_text
                            }),
                        )
                        .fill(if code_open { pal.accent } else { pal.btn_idle });
                        if ui.add(code_btn).clicked() {
                            self.right_drawer = if code_open {
                                None
                            } else {
                                Some(RightDrawerTab::CodeYaml)
                            };
                        }

                        if cfg!(feature = "plus") {
                            let help_open = self.right_drawer == Some(RightDrawerTab::Help);
                            let help_btn = egui::Button::new(
                                egui::RichText::new("Help").strong().color(if help_open {
                                    pal.accent_text
                                } else {
                                    pal.btn_idle_text
                                }),
                            )
                            .fill(if help_open { pal.accent } else { pal.btn_idle });
                            if ui.add(help_btn).clicked() {
                                self.right_drawer = if help_open {
                                    None
                                } else {
                                    Some(RightDrawerTab::Help)
                                };
                            }
                        }

                        let th_btn = egui::Button::new(
                            egui::RichText::new("Theme").strong().color(if self.show_theme_bar {
                                pal.accent_text
                            } else {
                                pal.btn_idle_text
                            }),
                        )
                        .fill(if self.show_theme_bar {
                            pal.accent
                        } else {
                            pal.btn_idle
                        });
                        if ui.add(th_btn).clicked() {
                            self.show_theme_bar = !self.show_theme_bar;
                        }

                        let hint_lbl = if self.show_plain_hints {
                            "Hints: ON"
                        } else {
                            "Hints: OFF"
                        };
                        let hint_fill = if self.show_plain_hints {
                            egui::Color32::from_rgb(16, 125, 78)
                        } else {
                            pal.btn_idle
                        };
                        let hint_txt = if self.show_plain_hints {
                            egui::Color32::WHITE
                        } else {
                            pal.btn_idle_text
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(hint_lbl).strong().color(hint_txt),
                                )
                                .fill(hint_fill),
                            )
                            .clicked()
                        {
                            self.show_plain_hints = !self.show_plain_hints;
                        }

                        let (em_lbl, em_fill, em_txt) = if edit_mode {
                            ("EDIT MODE: ON", pal.edit_hl, contrast_on(pal.edit_hl))
                        } else {
                            ("Edit Mode: OFF", pal.btn_idle, pal.btn_idle_text)
                        };
                        let em_btn = egui::Button::new(
                            egui::RichText::new(em_lbl).strong().color(em_txt),
                        )
                        .fill(em_fill)
                        .corner_radius(egui::CornerRadius::same(14));
                        if ui.add(em_btn).clicked() {
                            self.edit_mode = !self.edit_mode;
                            self.status_msg = if self.edit_mode {
                                "Edit Mode UNLOCKED — edit Routing Sets, Families, Sub-Profiles, Engines, and paths.".to_string()
                            } else {
                                "Edit Mode LOCKED — back to clean Daily Mode.".to_string()
                            };
                        }
                    });
                });

                // COLLAPSIBLE THEME BAR
                if self.show_theme_bar {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("Surface:").size(12.0).color(pal.muted));
                        let mut theme_changed = false;
                        theme_changed |= ui
                            .selectable_value(
                                &mut self.cfg.theme_mode,
                                ThemeMode::OledBlack,
                                "OLED Black (#000)",
                            )
                            .changed();
                        theme_changed |= ui
                            .selectable_value(
                                &mut self.cfg.theme_mode,
                                ThemeMode::Grey,
                                "Grey (Mid)",
                            )
                            .changed();
                        theme_changed |= ui
                            .selectable_value(
                                &mut self.cfg.theme_mode,
                                ThemeMode::Light,
                                "Warm Matte Light",
                            )
                            .changed();

                        ui.separator();
                        ui.label(egui::RichText::new("Accent:").size(12.0).color(pal.muted));
                        let presets: &[(&str, [u8; 3])] = &[
                            ("Blue", [59, 130, 246]),
                            ("Emerald", [16, 185, 129]),
                            ("Cyan", [6, 182, 212]),
                            ("Yellow", [250, 204, 21]),
                            ("Amber", [245, 158, 11]),
                            ("Rose", [244, 63, 94]),
                            ("Violet", [139, 92, 246]),
                        ];
                        for (name, rgb) in presets {
                            let c = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                            let selected = self.cfg.accent_rgb == *rgb;
                            let sw = egui::Button::new("")
                                .fill(c)
                                .min_size(egui::vec2(20.0, 20.0))
                                .stroke(if selected {
                                    egui::Stroke::new(2.0, pal.text)
                                } else {
                                    egui::Stroke::new(1.0, pal.card_border)
                                });
                            if ui.add(sw).on_hover_text(*name).clicked() {
                                self.cfg.accent_rgb = *rgb;
                                theme_changed = true;
                            }
                        }

                        let mut custom = egui::Color32::from_rgb(
                            self.cfg.accent_rgb[0],
                            self.cfg.accent_rgb[1],
                            self.cfg.accent_rgb[2],
                        );
                        if ui.color_edit_button_srgba(&mut custom).changed() {
                            self.cfg.accent_rgb = [custom.r(), custom.g(), custom.b()];
                            theme_changed = true;
                        }

                        if theme_changed {
                            self.save_now("Saved theme & accent preference.");
                        }
                    });
                }

                ui.add_space(6.0);
                // SERVICE & CONSOLE ROW
                ui.horizontal_wrapped(|ui| {
                    let llama_lbl = if procs.llama {
                        format!("Engine (:{}): ON", self.cfg.server_port)
                    } else {
                        format!("Engine (:{}): OFF", self.cfg.server_port)
                    };
                    let llama_fill = if procs.llama {
                        egui::Color32::from_rgb(16, 125, 78)
                    } else {
                        pal.btn_idle
                    };
                    let llama_txt = if procs.llama {
                        egui::Color32::WHITE
                    } else {
                        pal.btn_idle_text
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(llama_lbl).strong().color(llama_txt),
                            )
                            .fill(llama_fill)
                            .corner_radius(egui::CornerRadius::same(14)),
                        )
                        .clicked()
                    {
                        if procs.llama {
                            let exes = collect_backend_exes(&self.cfg);
                            stop_all_backends(&exes);
                            self.loaded_label = None;
                            procs.llama = false;
                            self.set_procs(procs);
                            self.status_msg =
                                "Stopped active inference engine — VRAM freed.".to_string();
                        } else if let Some(set) =
                            self.cfg.routing_sets.get(self.cfg.selected_set_idx).cloned()
                        {
                            self.sync_backend_exes();
                            match launch_routing_set(
                                &self.cfg,
                                &set,
                                Arc::clone(&self.llama_console),
                                Arc::clone(&self.error_logs),
                                console_notifier(ui.ctx().clone(), llama_vp_id()),
                            ) {
                                Ok(()) => {
                                    self.loaded_label =
                                        Some(format!("Routing Set: {} [{}]", set.name, set.mode));
                                    procs.llama = true;
                                    self.set_procs(procs);
                                    self.status_msg = format!(
                                        "Started Routing Set '{}' on :{}.",
                                        set.name, self.cfg.server_port
                                    );
                                }
                                Err(e) => self.push_error(e),
                            }
                        }
                    }

                    let fe_name = self.cfg.effective_frontend_name().to_string();
                    let fe_exe = self.cfg.effective_frontend_exe_name();
                    let owu_lbl = if procs.owu {
                        format!("{fe_name} (:{}): ON", self.cfg.owu_port)
                    } else {
                        format!("{fe_name} (:{}): OFF", self.cfg.owu_port)
                    };
                    let owu_fill = if procs.owu {
                        egui::Color32::from_rgb(16, 125, 78)
                    } else {
                        pal.btn_idle
                    };
                    let owu_txt = if procs.owu {
                        egui::Color32::WHITE
                    } else {
                        pal.btn_idle_text
                    };
                    if ui
                        .add(
                            egui::Button::new(egui::RichText::new(owu_lbl).strong().color(owu_txt))
                                .fill(owu_fill)
                                .corner_radius(egui::CornerRadius::same(14)),
                        )
                        .clicked()
                    {
                        if procs.owu {
                            stop_process(&fe_exe);
                            procs.owu = false;
                            self.set_procs(procs);
                            self.status_msg = format!("Stopped {fe_name}.");
                        } else {
                            match launch_open_webui(
                                &self.cfg,
                                Arc::clone(&self.owu_console),
                                Arc::clone(&self.error_logs),
                                console_notifier(ui.ctx().clone(), owu_vp_id()),
                            ) {
                                Ok(()) => {
                                    procs.owu = true;
                                    self.set_procs(procs);
                                    self.status_msg =
                                        format!("Started {fe_name} on port {}.", self.cfg.owu_port);
                                }
                                Err(e) => self.push_error(e),
                            }
                        }
                    }

                    if cfg!(feature = "plus") {
                        let comp_snap = self.cfg.companion_services.clone();
                        for (ci, svc) in comp_snap.iter().enumerate() {
                            if !svc.enabled || svc.name.trim().is_empty() {
                                continue;
                            }
                            let is_on =
                                ci < procs.companion_running.len() && procs.companion_running[ci];
                            let port_tag = if svc.check_port > 0 {
                                format!(" (:{})", svc.check_port)
                            } else {
                                String::new()
                            };
                            let lbl = if is_on {
                                format!("{}{port_tag}: ON", svc.name)
                            } else {
                                format!("{}{port_tag}: OFF", svc.name)
                            };
                            let fill = if is_on {
                                egui::Color32::from_rgb(16, 125, 78)
                            } else {
                                pal.btn_idle
                            };
                            let txt = if is_on {
                                egui::Color32::WHITE
                            } else {
                                pal.btn_idle_text
                            };
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new(lbl).strong().color(txt))
                                        .fill(fill)
                                        .corner_radius(egui::CornerRadius::same(14)),
                                )
                                .clicked()
                            {
                                let res = if is_on {
                                    stop_companion_service(svc)
                                } else {
                                    start_companion_service(svc)
                                };
                                match res {
                                    Ok(()) => {
                                        if ci < procs.companion_running.len() {
                                            procs.companion_running[ci] = !is_on;
                                        }
                                        self.set_procs(procs);
                                        self.status_msg = format!(
                                            "{} {}...",
                                            if is_on { "Stopping" } else { "Starting" },
                                            svc.name
                                        );
                                    }
                                    Err(e) => self.push_error(e),
                                }
                            }
                        }
                        if self.cfg.enable_mcpo_toggle {
                            let mcpo_lbl = if procs.mcpo {
                                format!("mcpo (:{}): ON", self.cfg.mcpo_port)
                            } else {
                                format!("mcpo (:{}): OFF", self.cfg.mcpo_port)
                            };
                            let mcpo_fill = if procs.mcpo {
                                egui::Color32::from_rgb(16, 125, 78)
                            } else {
                                pal.btn_idle
                            };
                            let mcpo_txt = if procs.mcpo {
                                egui::Color32::WHITE
                            } else {
                                pal.btn_idle_text
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(mcpo_lbl).strong().color(mcpo_txt),
                                    )
                                    .fill(mcpo_fill)
                                    .corner_radius(egui::CornerRadius::same(14)),
                                )
                                .clicked()
                            {
                                if procs.mcpo {
                                    stop_process(&platform::default_exe_name("mcpo"));
                                    procs.mcpo = false;
                                    self.set_procs(procs);
                                } else if let Err(e) = launch_mcpo(&self.cfg) {
                                    self.push_error(e);
                                } else {
                                    procs.mcpo = true;
                                    self.set_procs(procs);
                                }
                            }
                        }
                    }

                    ui.separator();
                    let llama_win_open = self.show_llama_console.load(Ordering::Relaxed);
                    let owu_win_open = self.show_owu_console.load(Ordering::Relaxed);
                    let any_console_open = llama_win_open || owu_win_open;
                    let fe_con_tag = if fe_name == "Open WebUI" {
                        "OWU"
                    } else {
                        "Frontend"
                    };

                    let l_fill = if llama_win_open {
                        pal.accent
                    } else {
                        pal.btn_idle
                    };
                    let l_txt = if llama_win_open {
                        pal.accent_text
                    } else {
                        pal.btn_idle_text
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(if llama_win_open {
                                    "Engine Console [ON]"
                                } else {
                                    "Engine Console"
                                })
                                .strong()
                                .color(l_txt),
                            )
                            .fill(l_fill),
                        )
                        .clicked()
                    {
                        self.show_llama_console
                            .store(!llama_win_open, Ordering::Relaxed);
                    }
                    let o_fill = if owu_win_open {
                        egui::Color32::from_rgb(16, 125, 78)
                    } else {
                        pal.btn_idle
                    };
                    let o_txt = if owu_win_open {
                        egui::Color32::WHITE
                    } else {
                        pal.btn_idle_text
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(if owu_win_open {
                                    format!("{fe_con_tag} Console [ON]")
                                } else {
                                    format!("{fe_con_tag} Console")
                                })
                                .strong()
                                .color(o_txt),
                            )
                            .fill(o_fill),
                        )
                        .clicked()
                    {
                        self.show_owu_console.store(!owu_win_open, Ordering::Relaxed);
                    }
                    if ui
                        .button(if any_console_open {
                            "Close Both Consoles"
                        } else {
                            "Open Both Consoles"
                        })
                        .clicked()
                    {
                        let next = !any_console_open;
                        self.show_llama_console.store(next, Ordering::Relaxed);
                        self.show_owu_console.store(next, Ordering::Relaxed);
                    }

                    ui.separator();
                    if ui.button("Open Browser").clicked() {
                        platform::open_url(&self.cfg.effective_frontend_url());
                    }
                    if cfg!(feature = "plus") {
                        if !self.cfg.update_owu_bat.trim().is_empty()
                            && ui.button("Update OWU").clicked()
                        {
                            if let Err(e) = run_bat_file(&self.cfg.update_owu_bat) {
                                self.push_error(e);
                            } else {
                                self.status_msg = "Launched Update OWU script.".to_string();
                            }
                        }
                        if !self.cfg.update_python_bat.trim().is_empty()
                            && ui.button("Update Python").clicked()
                        {
                            if let Err(e) = run_bat_file(&self.cfg.update_python_bat) {
                                self.push_error(e);
                            } else {
                                self.status_msg = "Launched Update Python script.".to_string();
                            }
                        }
                        for sc in self.cfg.quick_scripts.clone() {
                            if sc.label.trim().is_empty() || sc.path.trim().is_empty() {
                                continue;
                            }
                            if ui.button(&sc.label).clicked() {
                                if let Err(e) = run_bat_file(&sc.path) {
                                    self.push_error(e);
                                } else {
                                    self.status_msg = format!("Launched {}.", sc.label);
                                }
                            }
                        }
                    }
                    let stop_all = egui::Button::new(
                        egui::RichText::new("Stop All")
                            .strong()
                            .color(egui::Color32::WHITE),
                    )
                    .fill(egui::Color32::from_rgb(145, 32, 38));
                    if ui.add(stop_all).clicked() {
                        let exes = collect_backend_exes(&self.cfg);
                        stop_all_backends(&exes);
                        stop_process(&fe_exe);
                        stop_process(&platform::default_exe_name("mcpo"));
                        if cfg!(feature = "plus") {
                            for (ci, svc) in self.cfg.companion_services.iter().enumerate() {
                                if svc.enabled
                                    && ci < procs.companion_running.len()
                                    && procs.companion_running[ci]
                                {
                                    let _ = stop_companion_service(svc);
                                }
                            }
                        }
                        self.loaded_label = None;
                        self.set_procs(ProcStatus::default());
                        self.status_msg = "All services stopped. VRAM cleared.".to_string();
                    }
                });
            });

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
                                            let exes = collect_backend_exes(&self.cfg);
                                            stop_all_backends(&exes);
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
                                                                let mut dlg = FileDialog::new();
                                                                if cfg!(windows) {
                                                                    dlg = dlg.add_filter("Executable", &["exe"]);
                                                                }
                                                                if Path::new(start_dir).exists() {
                                                                    dlg = dlg.set_directory(start_dir);
                                                                }
                                                                if let Some(p) = dlg.pick_file() {
                                                                    if let Some(par) = p.parent() {
                                                                        prof.engine_dir_override = par.to_string_lossy().to_string();
                                                                    }
                                                                    if let Some(fname) = p.file_name() {
                                                                        prof.engine_exe_override = fname.to_string_lossy().to_string();
                                                                    }
                                                                    if prof.engine_label.trim().is_empty() {
                                                                        if let Some(stem) = p.file_stem() {
                                                                            prof.engine_label = stem.to_string_lossy().to_string();
                                                                        }
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
                            if ui.button("Clear Log").clicked() {
                                if let Ok(mut l) = self.error_logs.lock() {
                                    l.clear();
                                }
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