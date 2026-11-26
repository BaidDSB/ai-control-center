use super::theme::{
    default_engine_rgb, engine_badge_color_for, pasteable_singleline, themed_card, UiPalette,
    ENGINE_COLOR_PALETTE,
};
use crate::config::{
    self, guess_help_key, scan_gguf_models, slugify, EnginePreset, GgufInfo, ModelGroup,
    ModelProfile, RoutingSet, StackConfig, ThemeMode,
};
use crate::platform;
use eframe::egui;
use rfd::FileDialog;
use std::collections::HashMap;
use std::path::Path;

pub(crate) struct SidebarDrafts {
    pub new_set_name: String,
    pub new_group_name: String,
    pub new_profile_name: String,
    pub new_engine_label: String,
    pub new_engine_kind: String,
    pub new_engine_dir: String,
    pub new_engine_exe: String,
    pub new_engine_args: String,
    pub new_engine_color: [u8; 3],
}

impl SidebarDrafts {
    pub(crate) fn new() -> Self {
        Self {
            new_set_name: String::new(),
            new_group_name: String::new(),
            new_profile_name: String::new(),
            new_engine_label: String::new(),
            new_engine_kind: "llama_cpp".to_string(),
            new_engine_dir: String::new(),
            new_engine_exe: platform::default_exe_name("llama-server"),
            new_engine_args: String::new(),
            new_engine_color: ENGINE_COLOR_PALETTE[1],
        }
    }
}

#[derive(Default)]
pub(crate) struct SidebarOutcome {
    pub save_note: Option<String>,
    pub sync_exes: bool,
    pub reload_help: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_left_workspace(
    ui: &mut egui::Ui,
    left_workspace_w: f32,
    avail_height: f32,
    col_gap: f32,
    col1_w: f32,
    col2_w: f32,
    edit_mode: bool,
    pal: UiPalette,
    cfg: &mut StackConfig,
    saved_cfg: &mut StackConfig,
    drafts: &mut SidebarDrafts,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    status_msg: &mut String,
) -> SidebarOutcome {
    let mut outcome = SidebarOutcome::default();
    let is_light = cfg.theme_mode == ThemeMode::Light;
    let linked_col = if is_light {
        egui::Color32::from_rgb(14, 124, 78)
    } else {
        egui::Color32::from_rgb(52, 211, 153)
    };

    ui.allocate_ui_with_layout(
        egui::vec2(left_workspace_w, avail_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_width(left_workspace_w);
            ui.set_max_width(left_workspace_w);

            // TOP CARD: ROUTING SETS BAR
            themed_card(pal).show(ui, |ui| {
                let inner_w = (left_workspace_w - 26.0).max(200.0);
                ui.set_min_width(inner_w);
                ui.set_max_width(inner_w);
                let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };

                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("ROUTING SETS (OWU MULTI-ENGINE SWITCHING)")
                                .size(11.5)
                                .strong()
                                .color(hdr_col),
                        )
                        .selectable(false),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(s) = cfg.routing_sets.get(cfg.selected_set_idx) {
                            let mtag = if s.mode == "llama_ini" {
                                "models.ini"
                            } else {
                                "llama-swap"
                            };
                            ui.label(
                                egui::RichText::new(format!("Router: {mtag}"))
                                    .size(10.5)
                                    .color(pal.accent),
                            );
                        }
                    });
                });

                ui.add_space(4.0);

                // Set Selector Pills + Edit Mode Add/Del
                let mut select_set_idx: Option<usize> = None;
                let mut del_active_set = false;
                ui.horizontal_wrapped(|ui| {
                    for (sidx, set) in cfg.routing_sets.iter().enumerate() {
                        let is_active = sidx == cfg.selected_set_idx;
                        let pill = egui::Button::new(
                            egui::RichText::new(format!("{} ({})", set.name, set.profile_ids.len()))
                                .size(12.0)
                                .strong()
                                .color(if is_active {
                                    pal.accent_text
                                } else {
                                    pal.btn_idle_text
                                }),
                        )
                        .fill(if is_active { pal.accent } else { pal.btn_idle })
                        .corner_radius(egui::CornerRadius::same(8));
                        if ui.add(pill).on_hover_text(&set.description).clicked() {
                            select_set_idx = Some(sidx);
                        }
                    }

                    if edit_mode {
                        pasteable_singleline(ui, &mut drafts.new_set_name, 130.0, "New Set...");
                        if ui.button("+ Set").clicked() && !drafts.new_set_name.trim().is_empty() {
                            let name = drafts.new_set_name.trim().to_string();
                            let id = format!("{}-{}", slugify(&name), cfg.routing_sets.len() + 1);
                            cfg.routing_sets.push(RoutingSet {
                                id,
                                name: name.clone(),
                                description: "Cross-family multi-engine routing set.".to_string(),
                                mode: "llama_swap".to_string(),
                                no_models_autoload: false,
                                profile_ids: Vec::new(),
                            });
                            cfg.selected_set_idx = cfg.routing_sets.len() - 1;
                            drafts.new_set_name.clear();
                            outcome.save_note = Some(format!("Created Routing Set '{name}'."));
                        }
                        if cfg.routing_sets.len() > 1 {
                            let del_btn = egui::Button::new(
                                egui::RichText::new("Del").color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(145, 32, 38));
                            if ui
                                .add(del_btn)
                                .on_hover_text("Delete active Routing Set")
                                .clicked()
                            {
                                del_active_set = true;
                            }
                        }
                    }
                });

                if let Some(sidx) = select_set_idx {
                    cfg.selected_set_idx = sidx;
                    let _ = cfg.save();
                    *saved_cfg = cfg.clone();
                }
                if del_active_set && cfg.routing_sets.len() > 1 {
                    let removed = cfg.routing_sets.remove(cfg.selected_set_idx);
                    if cfg.selected_set_idx >= cfg.routing_sets.len() {
                        cfg.selected_set_idx = cfg.routing_sets.len() - 1;
                    }
                    outcome.save_note = Some(format!("Deleted Routing Set '{}'.", removed.name));
                }

                // Edit Active Set Details (Edit Mode only)
                if edit_mode {
                    if let Some(s_mut) = cfg.routing_sets.get_mut(cfg.selected_set_idx) {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Name:");
                            pasteable_singleline(ui, &mut s_mut.name, 145.0, "");
                            ui.label("Router:");
                            egui::ComboBox::from_id_salt("active_set_mode")
                                .selected_text(if s_mut.mode == "llama_ini" {
                                    "models.ini"
                                } else {
                                    "llama-swap"
                                })
                                .width(96.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut s_mut.mode,
                                        "llama_swap".to_string(),
                                        "llama-swap (Multi-Engine)",
                                    );
                                    ui.selectable_value(
                                        &mut s_mut.mode,
                                        "llama_ini".to_string(),
                                        "llama.cpp models.ini",
                                    );
                                });
                            if s_mut.mode == "llama_ini" {
                                ui.checkbox(&mut s_mut.no_models_autoload, "No Autoload");
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Desc:");
                            pasteable_singleline(
                                ui,
                                &mut s_mut.description,
                                (left_workspace_w - 94.0).max(120.0),
                                "",
                            );
                        });
                    }
                }

                ui.add_space(5.0);

                // Linked Model Chips (click chip -> jumps straight to Family & Sub-Profile)
                let mut unlink_req: Option<usize> = None;
                let mut move_link_req: Option<(usize, usize)> = None;
                let mut jump_to_pid: Option<String> = None;

                let cur_selected_pid = cfg
                    .current_profile()
                    .map(|p| p.id.clone())
                    .unwrap_or_default();

                if let Some(active_set) = cfg.routing_sets.get(cfg.selected_set_idx).cloned() {
                    if !edit_mode && !active_set.description.is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&active_set.description)
                                    .size(11.0)
                                    .color(pal.muted),
                            )
                            .selectable(false),
                        );
                        ui.add_space(4.0);
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("cc_routing_chips_scroll")
                        .max_height(135.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if active_set.profile_ids.is_empty() {
                                ui.label(
                                    egui::RichText::new("No profiles linked yet. Enable Edit Mode and click '+ Link' on any Sub-Profile below.")
                                        .size(11.0)
                                        .color(pal.muted),
                                );
                            } else {
                                let lcount = active_set.profile_ids.len();
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                    for (li, pid) in active_set.profile_ids.iter().enumerate() {
                                        let (short_label, hover_tip, eng_lbl) =
                                            match cfg.find_profile_with_group(pid) {
                                                Some((g, p)) => (
                                                    p.swap_model_id(),
                                                    format!(
                                                        "{} -> {} [{}]\nClick to select & inspect",
                                                        g.model_title,
                                                        p.name,
                                                        p.engine_badge_name()
                                                    ),
                                                    p.engine_badge_name().to_string(),
                                                ),
                                                None => (
                                                    pid.clone(),
                                                    "Missing profile reference".to_string(),
                                                    "missing".to_string(),
                                                ),
                                            };
                                        let ecol =
                                            engine_badge_color_for(&eng_lbl, &cfg.saved_engines);
                                        let is_cur = cur_selected_pid == *pid;
                                        let star = if li == 0 { "* " } else { "" };

                                        let mut job = egui::text::LayoutJob::default();
                                        job.append(
                                            &format!("{star}{short_label}\n"),
                                            0.0,
                                            egui::TextFormat {
                                                font_id: egui::FontId::proportional(11.5),
                                                color: pal.text,
                                                ..Default::default()
                                            },
                                        );
                                        job.append(
                                            &format!("[{eng_lbl}]"),
                                            0.0,
                                            egui::TextFormat {
                                                font_id: egui::FontId::proportional(10.0),
                                                color: ecol,
                                                ..Default::default()
                                            },
                                        );

                                        let chip_fill =
                                            if is_cur { pal.item_sel_bg } else { pal.code_bg };
                                        let chip_stroke = egui::Stroke::new(
                                            if is_cur { 1.5 } else { 1.0 },
                                            if is_cur { pal.item_sel_border } else { ecol },
                                        );

                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            let chip_btn = egui::Button::new(job)
                                                .fill(chip_fill)
                                                .stroke(chip_stroke)
                                                .corner_radius(egui::CornerRadius::same(7));
                                            if ui.add(chip_btn).on_hover_text(hover_tip).clicked() {
                                                jump_to_pid = Some(pid.clone());
                                            }
                                            if edit_mode {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 3.0;
                                                    if li > 0 && ui.small_button("<").clicked() {
                                                        move_link_req = Some((li, li - 1));
                                                    }
                                                    if li + 1 < lcount
                                                        && ui.small_button(">").clicked()
                                                    {
                                                        move_link_req = Some((li, li + 1));
                                                    }
                                                    if ui
                                                        .small_button("x")
                                                        .on_hover_text("Unlink")
                                                        .clicked()
                                                    {
                                                        unlink_req = Some(li);
                                                    }
                                                });
                                            }
                                        });
                                    }
                                });
                            }
                        });
                }

                if let Some(pid) = jump_to_pid {
                    if let Some((gi, pi)) = cfg.find_profile_pos(&pid) {
                        cfg.selected_group_idx = gi;
                        cfg.selected_profile_idx = pi;
                        if let Some(p) = cfg.groups.get(gi).and_then(|g| g.profiles.get(pi)) {
                            *status_msg = format!("Selected '{}' from Routing Set.", p.name);
                        }
                    }
                }
                if let Some(li) = unlink_req {
                    if let Some(set) = cfg.routing_sets.get_mut(cfg.selected_set_idx) {
                        set.profile_ids.remove(li);
                        outcome.save_note = Some("Unlinked profile from Routing Set.".to_string());
                    }
                }
                if let Some((a, b)) = move_link_req {
                    if let Some(set) = cfg.routing_sets.get_mut(cfg.selected_set_idx) {
                        set.profile_ids.swap(a, b);
                        outcome.save_note =
                            Some("Reordered linked profiles in Routing Set.".to_string());
                    }
                }
            });

            ui.add_space(8.0);
            let browser_h = ui.available_height().max(200.0);

            // BOTTOM TWO-COLUMN BROWSER: 1. MODEL FAMILIES | 2. SUB-PROFILES
            ui.horizontal_top(|ui| {
                // =========================================================
                // COLUMN 1: MODEL FAMILIES
                // =========================================================
                ui.allocate_ui_with_layout(
                    egui::vec2(col1_w, browser_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(col1_w);
                        ui.set_max_width(col1_w);
                        themed_card(pal).show(ui, |ui| {
                            let c1_in = (col1_w - 26.0).max(120.0);
                            ui.set_min_width(c1_in);
                            ui.set_max_width(c1_in);
                            egui::ScrollArea::vertical()
                                .id_salt("cc_families_scroll")
                                .max_height((browser_h - 24.0).max(120.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // EDIT MODE: PATHS & SAVED ENGINES
                                    if edit_mode {
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

                                    let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new("1. MODEL FAMILIES")
                                                .size(12.0)
                                                .strong()
                                                .color(hdr_col),
                                        )
                                        .selectable(false),
                                    );

                                    if edit_mode {
                                        ui.horizontal(|ui| {
                                            pasteable_singleline(
                                                ui,
                                                &mut drafts.new_group_name,
                                                (col1_w - 108.0).max(80.0),
                                                "New Family...",
                                            );
                                            if ui.button("+ Add").clicked()
                                                && !drafts.new_group_name.trim().is_empty()
                                            {
                                                let title = drafts.new_group_name.trim().to_string();
                                                let uid = cfg.make_unique_profile_id(&title);
                                                let mut default_prof = ModelProfile::default();
                                                default_prof.id = uid;
                                                default_prof.help_key = guess_help_key(&title, "");
                                                cfg.groups.push(ModelGroup {
                                                    model_title: title.clone(),
                                                    subtitle: "Custom GGUF Family".to_string(),
                                                    profiles: vec![default_prof],
                                                });
                                                cfg.selected_group_idx = cfg.groups.len() - 1;
                                                cfg.selected_profile_idx = 0;
                                                drafts.new_group_name.clear();
                                                outcome.save_note =
                                                    Some(format!("Added Model Family '{title}'."));
                                            }
                                        });
                                    }

                                    ui.add_space(4.0);
                                    let mut swap_groups: Option<(usize, usize)> = None;
                                    let mut del_group_idx: Option<usize> = None;
                                    let glen = cfg.groups.len();

                                    for idx in 0..glen {
                                        let is_sel = cfg.selected_group_idx == idx;
                                        let title = cfg.groups[idx].model_title.clone();
                                        let sub = cfg.groups[idx].subtitle.clone();
                                        let count = cfg.groups[idx].profiles.len();

                                        let (cfill, cborder) = if is_sel {
                                            (pal.item_sel_bg, pal.item_sel_border)
                                        } else {
                                            (pal.item_bg, pal.item_border)
                                        };

                                        egui::Frame::new()
                                            .fill(cfill)
                                            .stroke(egui::Stroke::new(
                                                if is_sel { 1.5 } else { 1.0 },
                                                cborder,
                                            ))
                                            .corner_radius(egui::CornerRadius::same(10))
                                            .inner_margin(egui::Margin::same(8))
                                            .show(ui, |ui| {
                                                ui.set_min_width((col1_w - 44.0).max(140.0));
                                                let inner = ui.vertical(|ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&title)
                                                                .strong()
                                                                .color(pal.text),
                                                        )
                                                        .selectable(false),
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(format!(
                                                                "{sub} ({count})"
                                                            ))
                                                            .size(11.0)
                                                            .color(pal.muted),
                                                        )
                                                        .selectable(false),
                                                    );
                                                });
                                                if ui
                                                    .interact(
                                                        inner.response.rect,
                                                        ui.scope_id().with(("cc_grp", idx)),
                                                        egui::Sense::click(),
                                                    )
                                                    .clicked()
                                                {
                                                    if cfg.selected_group_idx != idx {
                                                        cfg.selected_group_idx = idx;
                                                        cfg.selected_profile_idx = 0;
                                                        let _ = cfg.save();
                                                        *saved_cfg = cfg.clone();
                                                    }
                                                }

                                                if edit_mode {
                                                    ui.add_space(4.0);
                                                    ui.separator();
                                                    ui.horizontal(|ui| {
                                                        if ui
                                                            .add_enabled(
                                                                idx > 0,
                                                                egui::Button::new("^")
                                                                    .min_size(egui::vec2(24.0, 18.0)),
                                                            )
                                                            .clicked()
                                                        {
                                                            swap_groups = Some((idx, idx - 1));
                                                        }
                                                        if ui
                                                            .add_enabled(
                                                                idx + 1 < glen,
                                                                egui::Button::new("v")
                                                                    .min_size(egui::vec2(24.0, 18.0)),
                                                            )
                                                            .clicked()
                                                        {
                                                            swap_groups = Some((idx, idx + 1));
                                                        }
                                                        if glen > 1 {
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    let del_b = egui::Button::new(
                                                                        egui::RichText::new("Del")
                                                                            .size(11.0)
                                                                            .color(egui::Color32::WHITE),
                                                                    )
                                                                    .fill(egui::Color32::from_rgb(
                                                                        145, 32, 38,
                                                                    ));
                                                                    if ui.add(del_b).clicked() {
                                                                        del_group_idx = Some(idx);
                                                                    }
                                                                },
                                                            );
                                                        }
                                                    });
                                                }
                                            });
                                        ui.add_space(4.0);
                                    }

                                    if let Some((a, b)) = swap_groups {
                                        cfg.groups.swap(a, b);
                                        if cfg.selected_group_idx == a {
                                            cfg.selected_group_idx = b;
                                        } else if cfg.selected_group_idx == b {
                                            cfg.selected_group_idx = a;
                                        }
                                        outcome.save_note =
                                            Some("Reordered Model Families and saved.".to_string());
                                    }
                                    if let Some(di) = del_group_idx {
                                        if cfg.groups.len() > 1 {
                                            let removed = cfg.groups.remove(di);
                                            let removed_ids: Vec<String> = removed
                                                .profiles
                                                .iter()
                                                .map(|p| p.id.clone())
                                                .collect();
                                            for s in &mut cfg.routing_sets {
                                                s.profile_ids.retain(|id| !removed_ids.contains(id));
                                            }
                                            if cfg.selected_group_idx >= cfg.groups.len() {
                                                cfg.selected_group_idx = cfg.groups.len() - 1;
                                            }
                                            cfg.selected_profile_idx = 0;
                                            outcome.save_note = Some(format!(
                                                "Deleted Model Family '{}'.",
                                                removed.model_title
                                            ));
                                        }
                                    }
                                });
                        });
                    },
                );

                ui.add_space(col_gap);

                // =========================================================
                // COLUMN 2: SUB-PROFILES FOR SELECTED FAMILY
                // =========================================================
                ui.allocate_ui_with_layout(
                    egui::vec2(col2_w, browser_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(col2_w);
                        ui.set_max_width(col2_w);
                        themed_card(pal).show(ui, |ui| {
                            let c2_in = (col2_w - 26.0).max(120.0);
                            ui.set_min_width(c2_in);
                            ui.set_max_width(c2_in);
                            let g_idx = cfg.selected_group_idx;
                            let g_title = cfg
                                .groups
                                .get(g_idx)
                                .map(|g| g.model_title.clone())
                                .unwrap_or_default();
                            let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };
                            let sub_col = if edit_mode { pal.edit_hl } else { pal.accent };

                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new("2. SUB-PROFILES")
                                        .size(12.0)
                                        .strong()
                                        .color(hdr_col),
                                )
                                .selectable(false),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&g_title)
                                        .size(12.5)
                                        .strong()
                                        .color(sub_col),
                                )
                                .selectable(false),
                            );
                            ui.add_space(4.0);

                            if edit_mode {
                                ui.horizontal(|ui| {
                                    pasteable_singleline(
                                        ui,
                                        &mut drafts.new_profile_name,
                                        (col2_w - 108.0).max(80.0),
                                        "New sub-profile...",
                                    );
                                    if ui.button("+ Add").clicked()
                                        && !drafts.new_profile_name.trim().is_empty()
                                    {
                                        let pname = drafts.new_profile_name.trim().to_string();
                                        let new_id =
                                            cfg.make_unique_profile_id(&format!("{g_title}-{pname}"));
                                        if let Some(grp) = cfg.groups.get_mut(g_idx) {
                                            let mut base = grp
                                                .profiles
                                                .get(cfg.selected_profile_idx)
                                                .cloned()
                                                .unwrap_or_default();
                                            base.id = new_id;
                                            base.name = pname.clone();
                                            grp.profiles.push(base);
                                            cfg.selected_profile_idx = grp.profiles.len() - 1;
                                            drafts.new_profile_name.clear();
                                            outcome.save_note =
                                                Some(format!("Added sub-profile '{pname}'."));
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }

                            let active_links: Vec<String> = cfg
                                .routing_sets
                                .get(cfg.selected_set_idx)
                                .map(|s| s.profile_ids.clone())
                                .unwrap_or_default();

                            let scroll_trim = if edit_mode { 94.0 } else { 64.0 };
                            let mut swap_profs: Option<(usize, usize)> = None;
                            let mut dup_prof: Option<usize> = None;
                            let mut del_prof: Option<usize> = None;
                            let mut toggle_link_pid: Option<String> = None;

                            egui::ScrollArea::vertical()
                                .id_salt("cc_subprofiles_scroll")
                                .max_height((browser_h - scroll_trim).max(100.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if let Some(grp) = cfg.groups.get(g_idx) {
                                        let plen = grp.profiles.len();
                                        for pidx in 0..plen {
                                            let p = &grp.profiles[pidx];
                                            let is_sel = cfg.selected_profile_idx == pidx;
                                            let pid = p.id.clone();
                                            let pname = p.name.clone();
                                            let prole = p.role_tag.clone();
                                            let eng_badge = p.engine_badge_name().to_string();
                                            let eng_col = engine_badge_color_for(
                                                &eng_badge,
                                                &cfg.saved_engines,
                                            );
                                            let is_linked = active_links.contains(&pid);

                                            let (cfill, cborder) = if is_sel {
                                                (pal.sub_sel_bg, pal.sub_sel_border)
                                            } else {
                                                (pal.item_bg, pal.item_border)
                                            };

                                            egui::Frame::new()
                                                .fill(cfill)
                                                .stroke(egui::Stroke::new(
                                                    if is_sel { 1.5 } else { 1.0 },
                                                    cborder,
                                                ))
                                                .corner_radius(egui::CornerRadius::same(10))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.set_min_width((col2_w - 44.0).max(150.0));

                                                    let inner = ui.vertical(|ui| {
                                                        ui.add(
                                                            egui::Label::new(
                                                                egui::RichText::new(&pname)
                                                                    .strong()
                                                                    .color(pal.text),
                                                            )
                                                            .selectable(false),
                                                        );
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.add(
                                                                egui::Label::new(
                                                                    egui::RichText::new(format!(
                                                                        "[{eng_badge}]"
                                                                    ))
                                                                    .size(10.5)
                                                                    .strong()
                                                                    .color(eng_col),
                                                                )
                                                                .selectable(false),
                                                            );
                                                            if is_linked {
                                                                ui.add(
                                                                    egui::Label::new(
                                                                        egui::RichText::new(
                                                                            "• In Active Set",
                                                                        )
                                                                        .size(10.5)
                                                                        .color(linked_col),
                                                                    )
                                                                    .selectable(false),
                                                                );
                                                            }
                                                        });
                                                        if !prole.is_empty() {
                                                            ui.add(
                                                                egui::Label::new(
                                                                    egui::RichText::new(&prole)
                                                                        .size(11.0)
                                                                        .color(pal.muted),
                                                                )
                                                                .selectable(false),
                                                            );
                                                        }
                                                    });
                                                    if ui
                                                        .interact(
                                                            inner.response.rect,
                                                            ui.scope_id()
                                                                .with(("cc_subprof", g_idx, pidx)),
                                                            egui::Sense::click(),
                                                        )
                                                        .clicked()
                                                    {
                                                        cfg.selected_profile_idx = pidx;
                                                    }

                                                    if edit_mode {
                                                        ui.add_space(4.0);
                                                        ui.separator();
                                                        ui.horizontal_wrapped(|ui| {
                                                            let link_lbl = if is_linked {
                                                                "Unlink"
                                                            } else {
                                                                "+ Link"
                                                            };
                                                            let link_fill = if is_linked {
                                                                egui::Color32::from_rgb(16, 125, 78)
                                                            } else {
                                                                pal.btn_idle
                                                            };
                                                            let link_txt = if is_linked {
                                                                egui::Color32::WHITE
                                                            } else {
                                                                pal.btn_idle_text
                                                            };
                                                            if ui
                                                                .add(
                                                                    egui::Button::new(
                                                                        egui::RichText::new(
                                                                            link_lbl,
                                                                        )
                                                                        .size(11.0)
                                                                        .color(link_txt),
                                                                    )
                                                                    .fill(link_fill),
                                                                )
                                                                .clicked()
                                                            {
                                                                toggle_link_pid = Some(pid.clone());
                                                            }
                                                            if ui
                                                                .add_enabled(
                                                                    pidx > 0,
                                                                    egui::Button::new("^").min_size(
                                                                        egui::vec2(22.0, 18.0),
                                                                    ),
                                                                )
                                                                .clicked()
                                                            {
                                                                swap_profs = Some((pidx, pidx - 1));
                                                            }
                                                            if ui
                                                                .add_enabled(
                                                                    pidx + 1 < plen,
                                                                    egui::Button::new("v").min_size(
                                                                        egui::vec2(22.0, 18.0),
                                                                    ),
                                                                )
                                                                .clicked()
                                                            {
                                                                swap_profs = Some((pidx, pidx + 1));
                                                            }
                                                            if ui.small_button("Dup").clicked() {
                                                                dup_prof = Some(pidx);
                                                            }
                                                            if plen > 1 {
                                                                let del_b = egui::Button::new(
                                                                    egui::RichText::new("Del")
                                                                        .size(11.0)
                                                                        .color(egui::Color32::WHITE),
                                                                )
                                                                .fill(egui::Color32::from_rgb(
                                                                    145, 32, 38,
                                                                ));
                                                                if ui.add(del_b).clicked() {
                                                                    del_prof = Some(pidx);
                                                                }
                                                            }
                                                        });
                                                    }
                                                });
                                            ui.add_space(4.0);
                                        }
                                    }
                                });

                            if let Some(pid) = toggle_link_pid {
                                if let Some(set) = cfg.routing_sets.get_mut(cfg.selected_set_idx) {
                                    if let Some(pos) = set.profile_ids.iter().position(|x| x == &pid)
                                    {
                                        set.profile_ids.remove(pos);
                                        outcome.save_note = Some(
                                            "Unlinked sub-profile from active Routing Set."
                                                .to_string(),
                                        );
                                    } else {
                                        set.profile_ids.push(pid);
                                        outcome.save_note = Some(
                                            "Linked sub-profile to active Routing Set.".to_string(),
                                        );
                                    }
                                }
                            }
                            if let Some((a, b)) = swap_profs {
                                if let Some(grp) = cfg.groups.get_mut(g_idx) {
                                    grp.profiles.swap(a, b);
                                    if cfg.selected_profile_idx == a {
                                        cfg.selected_profile_idx = b;
                                    } else if cfg.selected_profile_idx == b {
                                        cfg.selected_profile_idx = a;
                                    }
                                    outcome.save_note =
                                        Some("Reordered sub-profiles and saved.".to_string());
                                }
                            }
                            if let Some(di) = dup_prof {
                                if let Some(orig) = cfg
                                    .groups
                                    .get(g_idx)
                                    .and_then(|g| g.profiles.get(di))
                                    .cloned()
                                {
                                    let new_id =
                                        cfg.make_unique_profile_id(&format!("{}-copy", orig.id));
                                    let mut copy = orig;
                                    copy.id = new_id;
                                    copy.name = format!("{} (Copy)", copy.name);
                                    if let Some(grp) = cfg.groups.get_mut(g_idx) {
                                        grp.profiles.insert(di + 1, copy);
                                        cfg.selected_profile_idx = di + 1;
                                        outcome.save_note =
                                            Some("Duplicated sub-profile.".to_string());
                                    }
                                }
                            }
                            if let Some(di) = del_prof {
                                let mut removed_info: Option<(String, String)> = None;
                                if let Some(grp) = cfg.groups.get_mut(g_idx) {
                                    if grp.profiles.len() > 1 {
                                        let removed = grp.profiles.remove(di);
                                        removed_info = Some((removed.id, removed.name));
                                        if cfg.selected_profile_idx >= grp.profiles.len() {
                                            cfg.selected_profile_idx = grp.profiles.len() - 1;
                                        }
                                    }
                                }
                                if let Some((rid, rname)) = removed_info {
                                    for s in &mut cfg.routing_sets {
                                        s.profile_ids.retain(|x| x != &rid);
                                    }
                                    outcome.save_note =
                                        Some(format!("Deleted sub-profile '{rname}'."));
                                }
                            }
                        });
                    },
                );
            });
        },
    );

    outcome
}