use super::console::{console_notifier, llama_vp_id, owu_vp_id};
use super::drawer::RightDrawerTab;
use super::theme::{contrast_on, themed_card, UiPalette};
use super::{effective_app_title, ControlCenterApp};
use crate::config::ThemeMode;
use crate::platform;
use crate::runtime::{launch_mcpo, launch_open_webui, launch_routing_set, run_bat_file, start_companion_service, stop_companion_service, stop_tracked_backend, stop_tracked_frontend, stop_tracked_mcpo, ProcStatus};
use eframe::egui;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl ControlCenterApp {
    pub(super) fn render_top_header(
        &mut self,
        ui: &mut egui::Ui,
        pal: UiPalette,
        mut procs: ProcStatus,
        err_count: usize,
        edit_mode: bool,
    ) -> ProcStatus {
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
                        stop_tracked_backend();
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
                        stop_tracked_frontend();
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
                                stop_tracked_mcpo();
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
                    stop_tracked_backend();
                    stop_tracked_frontend();
                    stop_tracked_mcpo();
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
        procs
    }
}
