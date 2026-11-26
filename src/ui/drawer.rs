use super::theme::{engine_badge_color_for, UiPalette};
use crate::config::{
    compute_cli_diff_with_rpc, guess_help_key, DiffKind, HelpMap, ModelProfile, StackConfig,
    ThemeMode,
};
use crate::runtime;
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RightDrawerTab {
    Help,
    CodeYaml,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodePreviewScope {
    RoutingSet,
    SelectedProfile,
}

#[derive(Default)]
pub(crate) struct DrawerOutcome {
    pub reload_help: bool,
    pub status_msg: Option<String>,
}

pub(crate) fn build_diff_layout_job(
    current: &ModelProfile,
    saved: &ModelProfile,
    models_dir: &str,
    cur_rpc: Option<&str>,
    sav_rpc: Option<&str>,
    theme_mode: ThemeMode,
) -> (egui::text::LayoutJob, Vec<String>, usize) {
    use egui::{Color32, FontId, TextFormat};
    let diff = compute_cli_diff_with_rpc(current, saved, models_dir, cur_rpc, sav_rpc);
    let mut job = egui::text::LayoutJob::default();

    let (base_col, add_col, mod_col, dim_col) = if theme_mode == ThemeMode::Light {
        (
            Color32::from_rgb(36, 33, 29),
            Color32::from_rgb(14, 124, 78),
            Color32::from_rgb(168, 98, 8),
            Color32::from_rgb(114, 108, 98),
        )
    } else {
        (
            Color32::from_rgb(230, 234, 244),
            Color32::from_rgb(52, 211, 153),
            Color32::from_rgb(251, 191, 36),
            Color32::from_rgb(130, 138, 155),
        )
    };

    let base_fmt = TextFormat {
        font_id: FontId::monospace(11.5),
        color: base_col,
        ..Default::default()
    };
    let add_fmt = TextFormat {
        font_id: FontId::monospace(11.5),
        color: add_col,
        ..Default::default()
    };
    let mod_fmt = TextFormat {
        font_id: FontId::monospace(11.5),
        color: mod_col,
        ..Default::default()
    };
    let dim_fmt = TextFormat {
        font_id: FontId::monospace(11.5),
        color: dim_col,
        ..Default::default()
    };

    if let Some(kind) = diff.env_pinned {
        let fmt = if kind == DiffKind::Added {
            add_fmt.clone()
        } else {
            mod_fmt.clone()
        };
        job.append("[ENV: GGML_CUDA_NO_PINNED=1] ", 0.0, fmt);
    }

    job.append(&diff.exe_name, 0.0, dim_fmt);
    for (tok, kind) in &diff.tokens {
        job.append(" ", 0.0, base_fmt.clone());
        let fmt = match kind {
            DiffKind::Added => add_fmt.clone(),
            DiffKind::Modified => mod_fmt.clone(),
            DiffKind::Unchanged => base_fmt.clone(),
        };
        job.append(tok, 0.0, fmt);
    }

    (job, diff.removed, diff.change_count)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_right_drawer(
    ui: &mut egui::Ui,
    pal: UiPalette,
    cfg: &StackConfig,
    saved_cfg: &StackConfig,
    help_map: &HelpMap,
    right_drawer: &mut Option<RightDrawerTab>,
    code_scope: &mut CodePreviewScope,
    live_code_select_mode: &mut bool,
    copied_code_at: &mut f64,
) -> DrawerOutcome {
    let mut outcome = DrawerOutcome::default();
    let Some(active_tab) = *right_drawer else {
        return outcome;
    };

    let mut close_drawer_req = false;
    let is_light = cfg.theme_mode == ThemeMode::Light;
    let copied_col = if is_light {
        egui::Color32::from_rgb(14, 124, 78)
    } else {
        egui::Color32::from_rgb(52, 211, 153)
    };
    let warn_col = if is_light {
        egui::Color32::from_rgb(168, 98, 8)
    } else {
        egui::Color32::from_rgb(251, 191, 36)
    };
    let removed_col = if is_light {
        egui::Color32::from_rgb(185, 28, 28)
    } else {
        egui::Color32::from_rgb(248, 113, 113)
    };

    egui::Panel::right("cc_right_drawer")
        .resizable(true)
        .default_size(400.0)
        .min_size(300.0)
        .frame(
            egui::Frame::new()
                .fill(pal.card_bg)
                .stroke(egui::Stroke::new(1.0, pal.card_border))
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let code_sel = active_tab == RightDrawerTab::CodeYaml;
                if cfg!(feature = "plus") {
                    let help_sel = active_tab == RightDrawerTab::Help;
                    let h_btn = egui::Button::new(
                        egui::RichText::new("Help").strong().color(if help_sel {
                            pal.accent_text
                        } else {
                            pal.btn_idle_text
                        }),
                    )
                    .fill(if help_sel { pal.accent } else { pal.btn_idle });
                    if ui.add(h_btn).clicked() {
                        *right_drawer = Some(RightDrawerTab::Help);
                    }
                }

                let c_btn = egui::Button::new(
                    egui::RichText::new("Code / YAML")
                        .strong()
                        .color(if code_sel {
                            pal.accent_text
                        } else {
                            pal.btn_idle_text
                        }),
                )
                .fill(if code_sel { pal.accent } else { pal.btn_idle });
                if ui.add(c_btn).clicked() {
                    *right_drawer = Some(RightDrawerTab::CodeYaml);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("X").on_hover_text("Close side panel").clicked() {
                        close_drawer_req = true;
                    }
                    if active_tab == RightDrawerTab::Help {
                        if ui
                            .button("Edit")
                            .on_hover_text("Open external help.md in editor")
                            .clicked()
                        {
                            runtime::open_existing_file_in_notepad(
                                &StackConfig::resolve_config_relative(&cfg.help_md_path),
                            );
                        }
                        if ui
                            .button("Reload")
                            .on_hover_text("Reload help.md from disk")
                            .clicked()
                        {
                            outcome.reload_help = true;
                        }
                    }
                });
            });
            ui.separator();

            let sel_pair: Option<(String, ModelProfile)> =
                cfg.groups.get(cfg.selected_group_idx).and_then(|g| {
                    g.profiles
                        .get(cfg.selected_profile_idx)
                        .map(|p| (g.model_title.clone(), p.clone()))
                });
            let sel_set = cfg.routing_sets.get(cfg.selected_set_idx).cloned();

            egui::ScrollArea::vertical()
                .id_salt("cc_right_drawer_scroll")
                .show(ui, |ui| match active_tab {
                    RightDrawerTab::Help => {
                        if let Some((g_title, prof)) = sel_pair {
                            let eng_tag = prof.help_engine_tag();
                            let hkey = if prof.help_key.trim().is_empty() {
                                guess_help_key(
                                    &format!("{g_title} {}", prof.name),
                                    &prof.single.model_rel_path,
                                )
                            } else {
                                prof.help_key.trim().to_ascii_lowercase()
                            };
                            let hvar = prof.help_variant.trim().to_ascii_lowercase();

                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{g_title} — {}", prof.name))
                                        .size(14.0)
                                        .strong()
                                        .color(pal.text),
                                )
                                .selectable(false),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "@model: {hkey}  |  @engine: {eng_tag}{}",
                                        if hvar.is_empty() {
                                            String::new()
                                        } else {
                                            format!("  |  @variant: {hvar}")
                                        }
                                    ))
                                    .size(11.0)
                                    .color(pal.muted),
                                )
                                .selectable(false),
                            );
                            ui.add_space(10.0);

                            let gen_txt =
                                help_map.get(&(hkey.clone(), "general".to_string(), String::new()));
                            let eng_txt = help_map
                                .get(&(hkey.clone(), eng_tag.to_string(), hvar.clone()))
                                .or_else(|| {
                                    help_map.get(&(
                                        hkey.clone(),
                                        eng_tag.to_string(),
                                        String::new(),
                                    ))
                                });
                            let global_eng = help_map.get(&(
                                "*".to_string(),
                                eng_tag.to_string(),
                                String::new(),
                            ));

                            ui.label(
                                egui::RichText::new("GENERAL MODEL NOTES")
                                    .size(11.5)
                                    .strong()
                                    .color(pal.accent),
                            );
                            ui.add_space(4.0);
                            egui::Frame::new()
                                .fill(pal.item_bg)
                                .stroke(egui::Stroke::new(1.0, pal.item_border))
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.label(
                                        gen_txt
                                            .map(|s| s.as_str())
                                            .unwrap_or("No general section found in help.md for this @model key."),
                                    );
                                });

                            ui.add_space(12.0);
                            let eng_col = engine_badge_color_for(eng_tag, &cfg.saved_engines);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} ENGINE NOTES",
                                    eng_tag.to_ascii_uppercase()
                                ))
                                .size(11.5)
                                .strong()
                                .color(eng_col),
                            );
                            ui.add_space(4.0);
                            egui::Frame::new()
                                .fill(pal.item_bg)
                                .stroke(egui::Stroke::new(1.0, eng_col))
                                .corner_radius(egui::CornerRadius::same(8))
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.label(
                                        eng_txt
                                            .map(|s| s.as_str())
                                            .unwrap_or("No engine-specific section found in help.md for this model + engine."),
                                    );
                                });

                            if let Some(gtxt) = global_eng {
                                ui.add_space(12.0);
                                ui.label(
                                    egui::RichText::new("GLOBAL ENGINE NOTES")
                                        .size(11.0)
                                        .strong()
                                        .color(pal.muted),
                                );
                                ui.add_space(4.0);
                                ui.label(gtxt);
                            }

                            ui.add_space(14.0);
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!("Source: {}", cfg.help_md_path))
                                    .size(10.5)
                                    .color(pal.muted),
                            );
                        } else {
                            ui.label("Select a Sub-Profile to view its external help.md notes.");
                        }
                    }
                    RightDrawerTab::CodeYaml => {
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(
                                code_scope,
                                CodePreviewScope::RoutingSet,
                                "Active Routing Set",
                            );
                            ui.selectable_value(
                                code_scope,
                                CodePreviewScope::SelectedProfile,
                                "Selected Profile CLI",
                            );
                        });
                        ui.add_space(6.0);

                        let now = ui.ctx().input(|i| i.time);
                        let recently_copied = (now - *copied_code_at) < 2.5;

                        match *code_scope {
                            CodePreviewScope::RoutingSet => {
                                if let Some(set) = sel_set {
                                    let is_ini = set.mode == "llama_ini";
                                    let code = if is_ini {
                                        cfg.build_routing_set_ini(&set)
                                    } else {
                                        cfg.build_routing_set_yaml(&set)
                                    };
                                    let hdr = if is_ini {
                                        format!("models.ini — {}", set.name)
                                    } else {
                                        format!("llama-swap.yaml — {}", set.name)
                                    };

                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            egui::RichText::new(&hdr)
                                                .strong()
                                                .size(12.0)
                                                .color(pal.accent),
                                        );
                                        if recently_copied {
                                            ui.label(
                                                egui::RichText::new("Copied!")
                                                    .strong()
                                                    .color(copied_col),
                                            );
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        let copy_btn = egui::Button::new(
                                            egui::RichText::new("Copy YAML/INI")
                                                .strong()
                                                .color(pal.accent_text),
                                        )
                                        .fill(pal.accent);
                                        if ui.add(copy_btn).clicked()
                                            && runtime::copy_to_clipboard(&code)
                                        {
                                            *copied_code_at = now;
                                            outcome.status_msg = Some(
                                                "Copied Routing Set config to clipboard!"
                                                    .to_string(),
                                            );
                                        }
                                        if ui.button("Notepad").clicked() {
                                            let fname = if is_ini {
                                                "models_preview.ini"
                                            } else {
                                                "llama_swap_preview.yaml"
                                            };
                                            runtime::open_text_in_notepad(fname, &code);
                                        }
                                    });
                                    ui.add_space(6.0);
                                    let mut slice: &str = code.as_str();
                                    ui.add(
                                        egui::TextEdit::multiline(&mut slice)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(28),
                                    );
                                }
                            }
                            CodePreviewScope::SelectedProfile => {
                                if let Some((_, prof)) = sel_pair {
                                    let saved_prof =
                                        saved_cfg.find_profile(&prof.id).unwrap_or(&prof);
                                    let cur_rpc = cfg.effective_rpc_for_profile(&prof.single, None);
                                    let sav_rpc = saved_cfg.effective_rpc_for_profile(&saved_prof.single, None);
                                    let (diff_job, removed_items, change_count) =
                                        build_diff_layout_job(
                                            &prof,
                                            saved_prof,
                                            &cfg.models_dir,
                                            cur_rpc.as_deref(),
                                            sav_rpc.as_deref(),
                                            cfg.theme_mode,
                                        );
                                    let full_code = prof.code_text_with_rpc(&cfg.models_dir, cur_rpc.as_deref());

                                    ui.horizontal_wrapped(|ui| {
                                        if change_count > 0 {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{change_count} unsaved flag changes (Green=Added, Amber=Modified)"
                                                ))
                                                .size(11.0)
                                                .color(warn_col),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("Matches saved config")
                                                    .size(11.0)
                                                    .color(pal.muted),
                                            );
                                        }
                                        if recently_copied {
                                            ui.label(
                                                egui::RichText::new("Copied!")
                                                    .strong()
                                                    .color(copied_col),
                                            );
                                        }
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        let copy_btn = egui::Button::new(
                                            egui::RichText::new("Copy CLI")
                                                .strong()
                                                .color(pal.accent_text),
                                        )
                                        .fill(pal.accent);
                                        if ui.add(copy_btn).clicked()
                                            && runtime::copy_to_clipboard(&full_code)
                                        {
                                            *copied_code_at = now;
                                            outcome.status_msg = Some(
                                                "Copied profile CLI to clipboard!".to_string(),
                                            );
                                        }
                                        if ui.button("Notepad").clicked() {
                                            runtime::open_text_in_notepad(
                                                "live_flags.txt",
                                                &full_code,
                                            );
                                        }
                                        let mode_lbl = if *live_code_select_mode {
                                            "Show Color Diff"
                                        } else {
                                            "Select Text"
                                        };
                                        if ui.button(mode_lbl).clicked() {
                                            *live_code_select_mode = !*live_code_select_mode;
                                        }
                                    });
                                    ui.add_space(6.0);

                                    egui::Frame::new()
                                        .fill(pal.code_bg)
                                        .stroke(egui::Stroke::new(1.0, pal.card_border))
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .inner_margin(egui::Margin::same(8))
                                        .show(ui, |ui| {
                                            if *live_code_select_mode {
                                                let mut slice: &str = full_code.as_str();
                                                ui.add(
                                                    egui::TextEdit::multiline(&mut slice)
                                                        .font(egui::TextStyle::Monospace)
                                                        .desired_width(f32::INFINITY)
                                                        .desired_rows(16),
                                                );
                                            } else {
                                                ui.add(egui::Label::new(diff_job).selectable(true));
                                                if !removed_items.is_empty() {
                                                    ui.add_space(6.0);
                                                    let rem_txt = format!(
                                                        "Removed: - {}",
                                                        removed_items.join("   - ")
                                                    );
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(rem_txt)
                                                                .monospace()
                                                                .size(11.0)
                                                                .strikethrough()
                                                                .color(removed_col),
                                                        )
                                                        .selectable(true),
                                                    );
                                                }
                                            }
                                        });
                                }
                            }
                        }
                    }
                });
        });

    if close_drawer_req {
        *right_drawer = None;
    }

    outcome
}