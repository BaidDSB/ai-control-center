use super::setup::render_setup_card;
use super::theme::{
    engine_badge_color_for, pasteable_singleline, themed_card, UiPalette,
    ENGINE_COLOR_PALETTE,
};
use crate::config::{
    guess_help_key, slugify, GgufInfo, ModelGroup,
    ModelProfile, RoutingSet, StackConfig, ThemeMode,
};
use crate::platform;
use eframe::egui;
use std::collections::HashMap;

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
    pub rpc_test_status: Option<(bool, String)>,
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
            rpc_test_status: None,
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
                                        render_setup_card(
                                            ui,
                                            pal,
                                            cfg,
                                            drafts,
                                            scanned_models,
                                            gguf_cache,
                                            &mut outcome,
                                        );
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
