use super::sidebar::{SidebarDrafts, SidebarOutcome};
use super::theme::{default_engine_rgb, pasteable_singleline, UiPalette, ENGINE_COLOR_PALETTE};
use crate::config::{self, scan_gguf_models, EnginePreset, GgufInfo, RpcFallbackPolicy, StackConfig};
use crate::runtime;
use crate::platform;
use eframe::egui;
use rfd::FileDialog;
use std::collections::HashMap;
use std::path::Path;

pub(crate) fn render_setup_card(
    ui: &mut egui::Ui,
    pal: UiPalette,
    cfg: &mut StackConfig,
    drafts: &mut SidebarDrafts,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    outcome: &mut SidebarOutcome,
) {
    let needs_setup = cfg.models_dir.trim().is_empty()
        && cfg.llama_dir.trim().is_empty();
    let setup_hdr_txt = if cfg!(feature = "plus") {
        "SETUP: Paths, Engines, Frontend & Plus"
    } else {
        "SETUP: Paths, Engines & Frontend"
    };
    egui::Frame::new()
        .fill(pal.item_sel_bg)
        .stroke(egui::Stroke::new(1.5, pal.edit_hl))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::CollapsingHeader::new(
                egui::RichText::new(setup_hdr_txt)
                    .size(12.0)
                    .strong()
                    .color(pal.edit_hl),
            )
            .default_open(needs_setup)
            .show(ui, |ui| {
                ui.label("llama-swap (Router):");
                ui.horizontal(|ui| {
                    pasteable_singleline(ui, &mut cfg.llama_swap_exe, 185.0, "");
                    if ui.button("...").clicked() {
                        let mut dlg = FileDialog::new();
                        if cfg!(windows) {
                            dlg = dlg.add_filter("Executable", &["exe"]);
                        }
                        if let Some(p) = dlg.pick_file() {
                            cfg.llama_swap_exe = p.to_string_lossy().to_string();
                            outcome.save_note = Some("Updated llama-swap path.".to_string());
                        }
                    }
                });

                ui.label("Global Engines Folder:");
                ui.horizontal(|ui| {
                    pasteable_singleline(ui, &mut cfg.llama_dir, 185.0, "");
                    if ui.button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_folder() {
                            cfg.llama_dir = p.to_string_lossy().to_string();
                            outcome.save_note = Some("Updated Global Engines folder.".to_string());
                        }
                    }
                });

                if cfg!(feature = "plus") {
                    ui.label("External help.md:");
                    ui.horizontal(|ui| {
                        pasteable_singleline(ui, &mut cfg.help_md_path, 185.0, "");
                        if ui.button("...").clicked() {
                            if let Some(p) = FileDialog::new()
                                .add_filter("Markdown", &["md", "txt"])
                                .pick_file()
                            {
                                cfg.help_md_path = p.to_string_lossy().to_string();
                                outcome.reload_help = true;
                                outcome.save_note = Some("Updated external help.md path.".to_string());
                            }
                        }
                    });
                    ui.separator();
                    egui::CollapsingHeader::new(
                        egui::RichText::new("Companion Services")
                            .size(10.5)
                            .strong()
                            .color(pal.edit_hl),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut del_svc: Option<usize> = None;
                        let mut svc_dirty = false;
                        for (si, svc) in cfg.companion_services.iter_mut().enumerate() {
                            egui::Frame::new()
                                .fill(pal.item_bg)
                                .stroke(egui::Stroke::new(1.0, pal.item_border))
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin::same(6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if ui.checkbox(&mut svc.enabled, "").on_hover_text("Show toggle pill in header").changed() {
                                            svc_dirty = true;
                                        }
                                        if pasteable_singleline(ui, &mut svc.name, 105.0, "Name (e.g. SearXNG)").lost_focus() {
                                            svc_dirty = true;
                                        }
                                        ui.label("Port:");
                                        if ui.add(egui::DragValue::new(&mut svc.check_port).speed(1.0)).changed() {
                                            svc_dirty = true;
                                        }
                                        if ui.small_button("X").clicked() {
                                            del_svc = Some(si);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Start:");
                                        if pasteable_singleline(ui, &mut svc.start_cmd, 150.0, "start script...").lost_focus() {
                                            svc_dirty = true;
                                        }
                                        if ui.small_button("...").clicked() {
                                            if let Some(p) = FileDialog::new().pick_file() {
                                                svc.start_cmd = p.to_string_lossy().to_string();
                                                svc_dirty = true;
                                            }
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Stop: ");
                                        if pasteable_singleline(ui, &mut svc.stop_cmd, 150.0, "stop script...").lost_focus() {
                                            svc_dirty = true;
                                        }
                                        if ui.small_button("...").clicked() {
                                            if let Some(p) = FileDialog::new().pick_file() {
                                                svc.stop_cmd = p.to_string_lossy().to_string();
                                                svc_dirty = true;
                                            }
                                        }
                                    });
                                });
                            ui.add_space(3.0);
                        }
                        if let Some(i) = del_svc {
                            cfg.companion_services.remove(i);
                            svc_dirty = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("+ Add Service").clicked() {
                                cfg.companion_services.push(config::CompanionService {
                                    name: "New Service".to_string(),
                                    start_cmd: String::new(),
                                    stop_cmd: String::new(),
                                    check_port: 8888,
                                    url: String::new(),
                                    enabled: true,
                                });
                                svc_dirty = true;
                            }
                            if ui.small_button("Save").clicked() {
                                svc_dirty = true;
                            }
                        });
                        if svc_dirty {
                            outcome.sync_exes = true;
                            outcome.save_note = Some("Updated Companion Services.".to_string());
                        }
                    });
                    egui::CollapsingHeader::new(
                        egui::RichText::new("Quick-Action Scripts")
                            .size(10.5)
                            .strong()
                            .color(pal.edit_hl),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        let mut del_sc: Option<usize> = None;
                        let mut sc_dirty = false;
                        for (qi, sc) in cfg.quick_scripts.iter_mut().enumerate() {
                            egui::Frame::new()
                                .fill(pal.item_bg)
                                .stroke(egui::Stroke::new(1.0, pal.item_border))
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin::same(6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if pasteable_singleline(ui, &mut sc.label, 170.0, "Button label...").lost_focus() {
                                            sc_dirty = true;
                                        }
                                        if ui.small_button("X").clicked() {
                                            del_sc = Some(qi);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        if pasteable_singleline(ui, &mut sc.path, 165.0, "script path...").lost_focus() {
                                            sc_dirty = true;
                                        }
                                        if ui.small_button("...").clicked() {
                                            if let Some(p) = FileDialog::new().pick_file() {
                                                sc.path = p.to_string_lossy().to_string();
                                                sc_dirty = true;
                                            }
                                        }
                                    });
                                });
                            ui.add_space(3.0);
                        }
                        if let Some(i) = del_sc {
                            cfg.quick_scripts.remove(i);
                            sc_dirty = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("+ Add Script").clicked() {
                                cfg.quick_scripts.push(config::QuickScript {
                                    label: "Run Script".to_string(),
                                    path: String::new(),
                                });
                                sc_dirty = true;
                            }
                            if ui.small_button("Save").clicked() {
                                sc_dirty = true;
                            }
                        });
                        if sc_dirty {
                            outcome.save_note = Some("Updated Quick-Action Scripts.".to_string());
                        }
                    });
                }

                ui.separator();
                ui.label(
                    egui::RichText::new("SAVED ENGINES LIBRARY")
                        .size(10.5)
                        .strong()
                        .color(pal.edit_hl),
                );
                let fallback_dir = cfg.llama_dir.clone();
                let mut del_eng_idx: Option<usize> = None;
                for (eidx, eng) in cfg.saved_engines.iter_mut().enumerate() {
                    egui::Frame::new()
                        .fill(pal.item_bg)
                        .stroke(egui::Stroke::new(1.0, pal.item_border))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let mut rgb = eng.color_rgb.unwrap_or_else(|| default_engine_rgb(&eng.label));
                                if ui.color_edit_button_srgb(&mut rgb).on_hover_text("Pick engine badge color").changed() {
                                    eng.color_rgb = Some(rgb);
                                    outcome.save_note = Some(format!("Updated color for '{}'.", eng.label));
                                }
                                pasteable_singleline(ui, &mut eng.label, 145.0, "Engine name...");
                                if ui.small_button("X").clicked() {
                                    del_eng_idx = Some(eidx);
                                }
                            });
                            ui.horizontal(|ui| {
                                let cm = if eng.engine_kind == "custom" { "custom" } else { "llama_cpp" };
                                egui::ComboBox::from_id_salt(("eng_mode", eidx))
                                    .selected_text(cm)
                                    .width(78.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut eng.engine_kind, "llama_cpp".to_string(), "llama_cpp");
                                        ui.selectable_value(&mut eng.engine_kind, "custom".to_string(), "custom");
                                    });
                                pasteable_singleline(ui, &mut eng.exe_name, 88.0, "exe...");
                                if ui.small_button(".exe").clicked() {
                                    let start = if eng.dir_path.trim().is_empty() {
                                        fallback_dir.as_str()
                                    } else {
                                        eng.dir_path.as_str()
                                    };
                                    let mut dlg = FileDialog::new();
                                    if cfg!(windows) {
                                        dlg = dlg.add_filter("Executable", &["exe"]);
                                    }
                                    if Path::new(start).exists() {
                                        dlg = dlg.set_directory(start);
                                    }
                                    if let Some(p) = dlg.pick_file() {
                                        if let Some(par) = p.parent() {
                                            eng.dir_path = par.to_string_lossy().to_string();
                                        }
                                        if let Some(fname) = p.file_name() {
                                            eng.exe_name = fname.to_string_lossy().to_string();
                                        }
                                        outcome.save_note = Some(format!("Updated engine '{}'.", eng.label));
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                pasteable_singleline(ui, &mut eng.dir_path, 178.0, "Folder...");
                                if ui.small_button("...").clicked() {
                                    let start = if eng.dir_path.trim().is_empty() {
                                        fallback_dir.as_str()
                                    } else {
                                        eng.dir_path.as_str()
                                    };
                                    let mut dlg = FileDialog::new();
                                    if Path::new(start).exists() {
                                        dlg = dlg.set_directory(start);
                                    }
                                    if let Some(p) = dlg.pick_folder() {
                                        eng.dir_path = p.to_string_lossy().to_string();
                                        outcome.save_note = Some(format!("Updated folder for '{}'.", eng.label));
                                    }
                                }
                            });
                            if eng.engine_kind == "custom" {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Args:").size(10.0).color(pal.muted));
                                    if pasteable_singleline(
                                        ui,
                                        &mut eng.default_args,
                                        165.0,
                                        "serve --model {model} --ctx {ctx} --addr {addr}",
                                    )
                                    .lost_focus()
                                    {
                                        outcome.save_note = Some(format!("Updated default args for '{}'.", eng.label));
                                    }
                                });
                            }
                        });
                    ui.add_space(3.0);
                }
                if let Some(di) = del_eng_idx {
                    let removed = cfg.saved_engines.remove(di);
                    outcome.save_note = Some(format!("Removed saved engine '{}'.", removed.label));
                }

                ui.add_space(4.0);
                ui.label(egui::RichText::new("Add New Engine:").size(10.5).strong());
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut drafts.new_engine_color)
                        .on_hover_text("Pick color for this engine");
                    pasteable_singleline(ui, &mut drafts.new_engine_label, 182.0, "Engine name...");
                });
                ui.horizontal(|ui| {
                    let cm = if drafts.new_engine_kind == "custom" { "custom" } else { "llama_cpp" };
                    egui::ComboBox::from_id_salt("new_eng_kind")
                        .selected_text(cm)
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut drafts.new_engine_kind, "llama_cpp".to_string(), "llama_cpp");
                            if ui
                                .selectable_value(&mut drafts.new_engine_kind, "custom".to_string(), "custom")
                                .clicked()
                                && drafts.new_engine_args.trim().is_empty()
                            {
                                drafts.new_engine_args = "serve --model {model} --ctx {ctx} --addr {addr}".to_string();
                            }
                        });
                    pasteable_singleline(ui, &mut drafts.new_engine_exe, 88.0, "exe...");
                    if ui.small_button(".exe").clicked() {
                        let mut dlg = FileDialog::new();
                        if cfg!(windows) {
                            dlg = dlg.add_filter("Executable", &["exe"]);
                        }
                        if let Some(p) = dlg.pick_file() {
                            if let Some(par) = p.parent() {
                                drafts.new_engine_dir = par.to_string_lossy().to_string();
                            }
                            if let Some(fname) = p.file_name() {
                                drafts.new_engine_exe = fname.to_string_lossy().to_string();
                            }
                            if drafts.new_engine_label.trim().is_empty() {
                                if let Some(stem) = p.file_stem() {
                                    drafts.new_engine_label = stem.to_string_lossy().to_string();
                                }
                            }
                        }
                    }
                });
                if drafts.new_engine_kind == "custom" {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Args:").size(10.0).color(pal.muted));
                        pasteable_singleline(
                            ui,
                            &mut drafts.new_engine_args,
                            175.0,
                            "serve --model {model} --ctx {ctx} --addr {addr}",
                        );
                    });
                }
                ui.horizontal(|ui| {
                    pasteable_singleline(ui, &mut drafts.new_engine_dir, 140.0, "Folder...");
                    if ui.small_button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_folder() {
                            drafts.new_engine_dir = p.to_string_lossy().to_string();
                        }
                    }
                    if ui.small_button("+ Save").clicked()
                        && !drafts.new_engine_label.trim().is_empty()
                        && !drafts.new_engine_dir.trim().is_empty()
                    {
                        let lbl = drafts.new_engine_label.trim().to_string();
                        let exe_n = if drafts.new_engine_exe.trim().is_empty() {
                            platform::default_exe_name("llama-server")
                        } else {
                            drafts.new_engine_exe.trim().to_string()
                        };
                        let def_args = if drafts.new_engine_kind == "custom" {
                            drafts.new_engine_args.trim().to_string()
                        } else {
                            String::new()
                        };
                        cfg.saved_engines.push(EnginePreset {
                            label: lbl.clone(),
                            engine_kind: drafts.new_engine_kind.clone(),
                            dir_path: drafts.new_engine_dir.trim().to_string(),
                            exe_name: exe_n,
                            color_rgb: Some(drafts.new_engine_color),
                            default_args: def_args,
                        });
                        let next_idx = cfg.saved_engines.len() % ENGINE_COLOR_PALETTE.len();
                        drafts.new_engine_color = ENGINE_COLOR_PALETTE[next_idx];
                        drafts.new_engine_label.clear();
                        drafts.new_engine_dir.clear();
                        drafts.new_engine_args.clear();
                        drafts.new_engine_exe = platform::default_exe_name("llama-server");
                        outcome.save_note = Some(format!("Saved engine '{lbl}'."));
                    }
                });

                ui.separator();
                ui.label("Models Folder (.gguf):");
                ui.horizontal(|ui| {
                    pasteable_singleline(ui, &mut cfg.models_dir, 185.0, "");
                    if ui.button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_folder() {
                            cfg.models_dir = p.to_string_lossy().to_string();
                            *scanned_models = scan_gguf_models(&cfg.models_dir);
                            gguf_cache.clear();
                            outcome.save_note = Some("Updated Models folder.".to_string());
                        }
                    }
                });
                ui.separator();
                ui.label(
                    egui::RichText::new("FRONTEND (CHAT UI)")
                        .size(10.5)
                        .strong()
                        .color(pal.edit_hl),
                );
                let mut fe_dirty = false;
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    if pasteable_singleline(ui, &mut cfg.frontend_name, 145.0, "e.g. Open WebUI, SillyTavern").lost_focus() {
                        fe_dirty = true;
                    }
                });
                ui.label("Frontend Executable:");
                ui.horizontal(|ui| {
                    if pasteable_singleline(ui, &mut cfg.frontend_exe, 185.0, "open-webui / script...").lost_focus() {
                        fe_dirty = true;
                    }
                    if ui.button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_file() {
                            cfg.frontend_exe = p.to_string_lossy().to_string();
                            fe_dirty = true;
                        }
                    }
                });
                ui.label("Frontend Folder (optional):");
                ui.horizontal(|ui| {
                    if pasteable_singleline(ui, &mut cfg.owu_scripts_dir, 185.0, "Fallback dir (e.g. Scripts/)").lost_focus() {
                        fe_dirty = true;
                    }
                    if ui.button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_folder() {
                            cfg.owu_scripts_dir = p.to_string_lossy().to_string();
                            fe_dirty = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Args:");
                    if pasteable_singleline(ui, &mut cfg.frontend_args, 175.0, "serve --port {port}").lost_focus() {
                        fe_dirty = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("URL: ");
                    if pasteable_singleline(ui, &mut cfg.frontend_url, 175.0, "http://127.0.0.1:{port}").lost_focus() {
                        fe_dirty = true;
                    }
                });
                if fe_dirty {
                    outcome.sync_exes = true;
                    outcome.save_note = Some("Updated Frontend settings.".to_string());
                }
                egui::CollapsingHeader::new(
                    egui::RichText::new("Secondary Node (LAN / RPC)")
                        .strong()
                        .color(pal.accent),
                )
                .default_open(cfg.secondary_node.enabled)
                .show(ui, |ui| {
                    let mut sn_dirty = false;
                    if ui
                        .checkbox(&mut cfg.secondary_node.enabled, "Enable Secondary Node")
                        .on_hover_text("Centralized LAN GPU node used by all RPC-enabled profiles (and llama-swap peers).")
                        .changed()
                    {
                        sn_dirty = true;
                        drafts.rpc_test_status = None;
                    }
                    ui.horizontal(|ui| {
                        ui.label("Label:");
                        if pasteable_singleline(ui, &mut cfg.secondary_node.label, 165.0, "Secondary Node").lost_focus() {
                            sn_dirty = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Host/IP:");
                        if pasteable_singleline(ui, &mut cfg.secondary_node.host, 155.0, "192.168.x.x").lost_focus() {
                            sn_dirty = true;
                            drafts.rpc_test_status = None;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("RPC Port:");
                        if ui.add(egui::DragValue::new(&mut cfg.secondary_node.rpc_port).range(1..=65535)).changed() {
                            sn_dirty = true;
                            drafts.rpc_test_status = None;
                        }
                        ui.label("Peer Port:").on_hover_text("HTTP port on the secondary node for llama-swap peer routing.");
                        if ui.add(egui::DragValue::new(&mut cfg.secondary_node.peer_port).range(1..=65535)).changed() {
                            sn_dirty = true;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("RPC Policy:")
                            .on_hover_text("Auto probes TCP (120ms) at launch and falls back locally if the node is off. Strict always passes --rpc.");
                        let prev_pol = cfg.secondary_node.rpc_fallback;
                        egui::ComboBox::from_id_salt("cc_setup_rpc_policy")
                            .width(155.0)
                            .selected_text(cfg.secondary_node.rpc_fallback.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut cfg.secondary_node.rpc_fallback,
                                    RpcFallbackPolicy::Auto,
                                    RpcFallbackPolicy::Auto.label(),
                                );
                                ui.selectable_value(
                                    &mut cfg.secondary_node.rpc_fallback,
                                    RpcFallbackPolicy::Strict,
                                    RpcFallbackPolicy::Strict.label(),
                                );
                            });
                        if cfg.secondary_node.rpc_fallback != prev_pol {
                            sn_dirty = true;
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        let host_t = cfg.secondary_node.host.trim();
                        let can_test = !host_t.is_empty() && cfg.secondary_node.rpc_port > 0;
                        if ui
                            .add_enabled(can_test, egui::Button::new("Test RPC"))
                            .on_hover_text("Quick 350ms TCP connect test against Host:RPC Port")
                            .clicked()
                        {
                            let ep = format!("{}:{}", host_t, cfg.secondary_node.rpc_port);
                            let ok = runtime::probe_tcp_endpoint(&ep, 350);
                            drafts.rpc_test_status = Some(if ok {
                                (true, format!("Online ({ep})"))
                            } else {
                                (false, format!("Offline ({ep})"))
                            });
                        }
                        if let Some((ok, msg)) = &drafts.rpc_test_status {
                            let col = if *ok {
                                egui::Color32::from_rgb(52, 211, 153)
                            } else {
                                egui::Color32::from_rgb(248, 113, 113)
                            };
                            ui.label(egui::RichText::new(msg).size(11.0).strong().color(col));
                        }
                    });
                    if sn_dirty {
                        outcome.save_note = Some("Updated Secondary Node settings.".to_string());
                    }
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("API:");
                    ui.add(egui::DragValue::new(&mut cfg.server_port).range(1024..=65535));
                    ui.label("Frontend:");
                    ui.add(egui::DragValue::new(&mut cfg.owu_port).range(1024..=65535));
                    if cfg!(feature = "plus") {
                        ui.checkbox(&mut cfg.enable_mcpo_toggle, "mcpo");
                    }
                });
                if ui.checkbox(&mut cfg.use_external_cmd_windows, "Use External CMD").changed() {
                    outcome.save_note = Some("Saved console mode preference.".to_string());
                }
            });
        });
    ui.add_space(10.0);
}
