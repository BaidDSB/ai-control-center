use crate::{config, platform, runtime, AppPatch};

use config::{
    build_diff_layout_job, guess_help_key, parse_help_markdown, parse_help_markdown_str, probe_gguf_header,
    scan_gguf_models, slugify, EnginePreset, GgufInfo, HelpMap, ModelGroup, ModelProfile,
    ModelSpec, RoutingSet, StackConfig, ThemeMode,
};
use eframe::egui;
use rfd::FileDialog;
use runtime::{
    collect_backend_exes, launch_mcpo, launch_open_webui, launch_routing_set,
    launch_single_profile, poll_processes_full, run_bat_file, start_companion_service, stop_companion_service, stop_all_backends, stop_process,
    ConsoleBuffer, ProcStatus, SharedConsole, SharedErrors,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;


#[derive(Clone, Copy, PartialEq, Eq)]
enum RightDrawerTab {
    Help,
    CodeYaml,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CodePreviewScope {
    RoutingSet,
    SelectedProfile,
}

#[derive(Clone, Copy)]
struct UiPalette {
    canvas: egui::Color32,
    card_bg: egui::Color32,
    card_border: egui::Color32,
    item_bg: egui::Color32,
    item_border: egui::Color32,
    item_sel_bg: egui::Color32,
    item_sel_border: egui::Color32,
    sub_sel_bg: egui::Color32,
    sub_sel_border: egui::Color32,
    code_bg: egui::Color32,
    text: egui::Color32,
    muted: egui::Color32,
    accent: egui::Color32,
    accent_text: egui::Color32,
    btn_idle: egui::Color32,
    btn_idle_text: egui::Color32,
    edit_hl: egui::Color32,
}

fn contrast_on(c: egui::Color32) -> egui::Color32 {
    let luma = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if luma > 155.0 {
        egui::Color32::from_rgb(14, 16, 20)
    } else {
        egui::Color32::WHITE
    }
}

fn blend_color(base: egui::Color32, tint: egui::Color32, factor: f32) -> egui::Color32 {
    let f = factor.clamp(0.0, 1.0);
    let r = (base.r() as f32 * (1.0 - f) + tint.r() as f32 * f).round() as u8;
    let g = (base.g() as f32 * (1.0 - f) + tint.g() as f32 * f).round() as u8;
    let b = (base.b() as f32 * (1.0 - f) + tint.b() as f32 * f).round() as u8;
    egui::Color32::from_rgb(r, g, b)
}

fn palette_for(mode: ThemeMode, accent_rgb: [u8; 3], edit_mode: bool) -> UiPalette {
    let accent = egui::Color32::from_rgb(accent_rgb[0], accent_rgb[1], accent_rgb[2]);
    let accent_text = contrast_on(accent);
    let w = egui::Color32::WHITE;
    let em = egui::Color32::from_rgb(16, 185, 129);
    match mode {
        ThemeMode::OledBlack if edit_mode => UiPalette {
            canvas: egui::Color32::BLACK, card_bg: egui::Color32::from_rgb(14, 14, 16), card_border: w,
            item_bg: egui::Color32::from_rgb(20, 20, 24), item_border: egui::Color32::from_rgb(125, 125, 135),
            item_sel_bg: egui::Color32::from_rgb(42, 44, 52), item_sel_border: w,
            sub_sel_bg: egui::Color32::from_rgb(42, 44, 52), sub_sel_border: w,
            code_bg: egui::Color32::from_rgb(9, 9, 11), text: w, muted: egui::Color32::from_rgb(190, 194, 204),
            accent, accent_text, btn_idle: egui::Color32::from_rgb(32, 34, 40), btn_idle_text: w, edit_hl: w,
        },
        ThemeMode::OledBlack => UiPalette {
            canvas: egui::Color32::BLACK, card_bg: egui::Color32::from_rgb(12, 12, 17), card_border: egui::Color32::from_rgb(36, 38, 52),
            item_bg: egui::Color32::from_rgb(18, 18, 25), item_border: egui::Color32::from_rgb(36, 36, 48),
            item_sel_bg: blend_color(egui::Color32::from_rgb(14, 16, 24), accent, 0.28), item_sel_border: accent,
            sub_sel_bg: egui::Color32::from_rgb(20, 56, 48), sub_sel_border: em,
            code_bg: egui::Color32::from_rgb(8, 9, 14), text: egui::Color32::from_rgb(236, 238, 244), muted: egui::Color32::from_rgb(154, 162, 178),
            accent, accent_text, btn_idle: egui::Color32::from_rgb(28, 30, 42), btn_idle_text: egui::Color32::from_rgb(230, 234, 242), edit_hl: w,
        },
        ThemeMode::Grey if edit_mode => UiPalette {
            canvas: egui::Color32::from_rgb(30, 32, 36), card_bg: egui::Color32::from_rgb(42, 44, 50), card_border: w,
            item_bg: egui::Color32::from_rgb(50, 53, 60), item_border: egui::Color32::from_rgb(145, 149, 160),
            item_sel_bg: egui::Color32::from_rgb(68, 72, 82), item_sel_border: w,
            sub_sel_bg: egui::Color32::from_rgb(68, 72, 82), sub_sel_border: w,
            code_bg: egui::Color32::from_rgb(24, 26, 30), text: w, muted: egui::Color32::from_rgb(194, 198, 208),
            accent, accent_text, btn_idle: egui::Color32::from_rgb(58, 62, 70), btn_idle_text: w, edit_hl: w,
        },
        ThemeMode::Grey => UiPalette {
            canvas: egui::Color32::from_rgb(30, 33, 39), card_bg: egui::Color32::from_rgb(40, 44, 52), card_border: egui::Color32::from_rgb(64, 70, 82),
            item_bg: egui::Color32::from_rgb(48, 53, 62), item_border: egui::Color32::from_rgb(70, 76, 90),
            item_sel_bg: blend_color(egui::Color32::from_rgb(44, 49, 58), accent, 0.28), item_sel_border: accent,
            sub_sel_bg: blend_color(egui::Color32::from_rgb(40, 50, 48), em, 0.24), sub_sel_border: em,
            code_bg: egui::Color32::from_rgb(24, 27, 32), text: egui::Color32::from_rgb(234, 237, 242), muted: egui::Color32::from_rgb(162, 170, 184),
            accent, accent_text, btn_idle: egui::Color32::from_rgb(54, 59, 70), btn_idle_text: egui::Color32::from_rgb(236, 240, 246), edit_hl: w,
        },
        ThemeMode::Light if edit_mode => {
            let ink = egui::Color32::from_rgb(24, 28, 36);
            UiPalette {
                canvas: egui::Color32::from_rgb(238, 240, 244), card_bg: w, card_border: ink,
                item_bg: w, item_border: egui::Color32::from_rgb(148, 154, 166),
                item_sel_bg: egui::Color32::from_rgb(230, 234, 242), item_sel_border: ink,
                sub_sel_bg: egui::Color32::from_rgb(230, 234, 242), sub_sel_border: ink,
                code_bg: egui::Color32::from_rgb(245, 247, 250), text: ink, muted: egui::Color32::from_rgb(88, 96, 110),
                accent, accent_text, btn_idle: egui::Color32::from_rgb(234, 238, 244), btn_idle_text: ink, edit_hl: ink,
            }
        }
        ThemeMode::Light => UiPalette {
            canvas: egui::Color32::from_rgb(240, 242, 246), card_bg: w, card_border: egui::Color32::from_rgb(208, 214, 224),
            item_bg: egui::Color32::from_rgb(246, 248, 251), item_border: egui::Color32::from_rgb(212, 218, 228),
            item_sel_bg: blend_color(w, accent, 0.14), item_sel_border: accent,
            sub_sel_bg: blend_color(w, em, 0.14), sub_sel_border: em,
            code_bg: egui::Color32::from_rgb(244, 246, 250), text: egui::Color32::from_rgb(22, 26, 34), muted: egui::Color32::from_rgb(96, 106, 122),
            accent, accent_text, btn_idle: egui::Color32::from_rgb(230, 234, 241), btn_idle_text: egui::Color32::from_rgb(26, 30, 38), edit_hl: egui::Color32::from_rgb(24, 28, 36),
        },
    }
}

const ENGINE_COLOR_PALETTE: [[u8; 3]; 8] = [
    [59, 130, 246],  // Blue
    [234, 120, 48],  // Orange
    [16, 185, 129],  // Emerald
    [168, 85, 247],  // Purple
    [6, 182, 212],   // Cyan
    [244, 63, 94],   // Rose
    [234, 179, 8],   // Gold
    [129, 140, 248], // Indigo
];

fn default_engine_rgb(label_or_tag: &str) -> [u8; 3] {
    let low = label_or_tag.trim().to_ascii_lowercase();
    if low.is_empty() || low.contains("llama") || low.contains("global") {
        return ENGINE_COLOR_PALETTE[0];
    }
    if low.contains("rusting") {
        return ENGINE_COLOR_PALETTE[1];
    }
    if low.contains("turbo") {
        return ENGINE_COLOR_PALETTE[2];
    }
    let hash = low.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    ENGINE_COLOR_PALETTE[1 + (hash % (ENGINE_COLOR_PALETTE.len() - 1))]
}

fn engine_badge_color_for(label_or_tag: &str, saved_engines: &[EnginePreset]) -> egui::Color32 {
    let trimmed = label_or_tag.trim();
    if !trimmed.is_empty() {
        let low = trimmed.to_ascii_lowercase();
        if let Some(eng) = saved_engines.iter().find(|e| e.label.trim().eq_ignore_ascii_case(trimmed))
            .or_else(|| saved_engines.iter().find(|e| {
                let el = e.label.trim().to_ascii_lowercase();
                !el.is_empty() && (el.contains(&low) || low.contains(&el))
            }))
        {
            if let Some([r, g, b]) = eng.color_rgb {
                return egui::Color32::from_rgb(r, g, b);
            }
        }
    }
    let [r, g, b] = default_engine_rgb(label_or_tag);
    egui::Color32::from_rgb(r, g, b)
}

fn llama_vp_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("ai_cc_llama_console_vp")
}

fn owu_vp_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("ai_cc_owu_console_vp")
}

fn load_window_icon(icon_path: &str, embedded: Option<&[u8]>) -> Option<egui::IconData> {
    let bytes = fs::read(icon_path.trim()).ok().or_else(|| embedded.map(|b| b.to_vec()))?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

fn pasteable_singleline(ui: &mut egui::Ui, value: &mut String, width: f32, hint: &str) -> egui::Response {
    let mut edit = egui::TextEdit::singleline(value).desired_width(width);
    if !hint.is_empty() {
        edit = edit.hint_text(hint);
    }
    let resp = ui.add(edit);
    if resp.has_focus() {
        let (paste_pressed, copy_pressed, had_native_paste) = ui.ctx().input(|i| {
            let mod_key = i.modifiers.ctrl || i.modifiers.command;
            let paste = mod_key && i.key_pressed(egui::Key::V);
            let copy = mod_key && i.key_pressed(egui::Key::C);
            let native_p = i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)));
            (paste, copy, native_p)
        });
        if paste_pressed && !had_native_paste {
            if let Some(clip) = runtime::read_from_clipboard() {
                let cleaned = clip.replace(['\r', '\n'], " ");
                let trimmed = cleaned.trim();
                if !trimmed.is_empty() {
                    if !value.is_empty() && !value.ends_with(' ') && !value.ends_with('\\') {
                        value.push(' ');
                    }
                    value.push_str(trimmed);
                }
            }
        }
        if copy_pressed && !value.is_empty() {
            let _ = runtime::copy_to_clipboard(value);
        }
    }
    resp
}

fn apply_app_theme(ctx: &egui::Context, mode: ThemeMode, accent_rgb: [u8; 3], edit_mode: bool) {
    let pal = palette_for(mode, accent_rgb, edit_mode);
    let egui_theme = if mode == ThemeMode::Light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    ctx.set_theme(egui_theme);

    let mut visuals = if mode == ThemeMode::Light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };

    let active_accent = if edit_mode { pal.edit_hl } else { pal.accent };
    let active_text = contrast_on(active_accent);

    visuals.window_fill = pal.canvas;
    visuals.panel_fill = pal.canvas;
    visuals.faint_bg_color = pal.card_bg;
    visuals.extreme_bg_color = pal.code_bg;
    visuals.code_bg_color = pal.code_bg;
    visuals.override_text_color = Some(pal.text);
    visuals.hyperlink_color = active_accent;

    visuals.widgets.noninteractive.bg_fill = pal.card_bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, pal.card_border);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, pal.text);
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = pal.btn_idle;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, pal.item_border);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, pal.btn_idle_text);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = blend_color(pal.btn_idle, active_accent, 0.22);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, active_accent);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, pal.text);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);

    visuals.widgets.active.bg_fill = active_accent;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, active_accent);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.5, active_text);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);

    visuals.selection.bg_fill = active_accent;
    visuals.selection.stroke = egui::Stroke::new(1.0, active_text);
    visuals.window_stroke = egui::Stroke::new(1.0, pal.card_border);
    visuals.window_corner_radius = egui::CornerRadius::same(12);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style_of(egui_theme)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.window_margin = egui::Margin::same(14);
    ctx.set_style_of(egui_theme, style);
}

fn themed_card(pal: UiPalette) -> egui::Frame {
    egui::Frame::new()
        .fill(pal.card_bg)
        .stroke(egui::Stroke::new(1.0, pal.card_border))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

fn pill_switch(
    ui: &mut egui::Ui,
    value: &mut bool,
    on_text: &str,
    off_text: &str,
    edit_mode: bool,
    pal: UiPalette,
    tip: &str,
) -> bool {
    let label = if *value { on_text } else { off_text };
    let fill = if *value { if edit_mode { pal.edit_hl } else { egui::Color32::from_rgb(16, 125, 78) } } else { pal.btn_idle };
    let text_color = if *value { contrast_on(fill) } else { pal.btn_idle_text };
    let btn = egui::Button::new(egui::RichText::new(label).strong().color(text_color))
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(14));
    let resp = ui.add(btn);
    let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

fn plain_help_row(ui: &mut egui::Ui, show_hints: bool, pal: UiPalette, tags: &[(&str, egui::Color32)], text: &str) {
    if !show_hints { return; }
    ui.horizontal_wrapped(|ui| {
        for (tag, col) in tags {
            ui.add(egui::Label::new(egui::RichText::new(format!("[{tag}]")).size(10.5).strong().color(*col)).selectable(false));
        }
        ui.add(egui::Label::new(egui::RichText::new(text).size(11.0).color(pal.muted)).selectable(false));
    });
}

fn render_console_viewport(
    ctx: &egui::Context,
    vp_id: egui::ViewportId,
    title: &'static str,
    accent: egui::Color32,
    open_flag: Arc<AtomicBool>,
    console: SharedConsole,
    procs: Arc<Mutex<ProcStatus>>,
    is_llama: bool,
) {
    if !open_flag.load(Ordering::Relaxed) {
        return;
    }

    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([880.0, 500.0])
        .with_min_inner_size([520.0, 280.0]);

    ctx.show_viewport_deferred(vp_id, builder, move |ui, _class| {
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            open_flag.store(false, Ordering::Relaxed);
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            return;
        }

        let running = procs
            .lock()
            .map(|p| if is_llama { p.llama } else { p.owu })
            .unwrap_or(false);

        let frame = egui::Frame::new()
            .fill(egui::Color32::BLACK)
            .inner_margin(egui::Margin::same(10));

        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let mut copy_all_req = false;
            let mut copy_last_req = false;
            let mut notepad_req = false;
            let mut clear_req = false;
            let mut hide_req = false;

            let copy_id = egui::Id::new((vp_id, "copied_at"));
            let now = ui.ctx().input(|i| i.time);
            let recently_copied = ui
                .ctx()
                .data(|d| d.get_temp::<f64>(copy_id))
                .map(|t| (now - t) < 2.5)
                .unwrap_or(false);

            let (auto_scroll, filter_lower, filtered_lines, total_lines, full_text) = {
                let mut guard = console.lock().unwrap();
                ui.horizontal_wrapped(|ui| {
                    let (st_txt, st_col) = if running {
                        ("LIVE", egui::Color32::from_rgb(52, 211, 153))
                    } else {
                        ("STOPPED", egui::Color32::from_rgb(251, 191, 36))
                    };
                    ui.add(egui::Label::new(egui::RichText::new(format!("[{st_txt}]")).strong().color(st_col)).selectable(false));
                    ui.add(egui::Label::new(egui::RichText::new(title).strong().color(accent)).selectable(false));
                    ui.separator();
                    pasteable_singleline(ui, &mut guard.filter, 150.0, "Filter logs...");
                    ui.checkbox(&mut guard.auto_scroll, "Auto-scroll");

                    if recently_copied {
                        ui.add(egui::Label::new(egui::RichText::new("Copied!").strong().color(egui::Color32::from_rgb(52, 211, 153))).selectable(false));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Hide").clicked() { hide_req = true; }
                        if ui.button("Clear").clicked() { clear_req = true; }
                        if ui.button("Notepad").clicked() { notepad_req = true; }
                        let copy_all_btn = egui::Button::new(egui::RichText::new("Copy All").strong().color(egui::Color32::WHITE))
                            .fill(egui::Color32::from_rgb(37, 99, 235));
                        if ui.add(copy_all_btn).clicked() { copy_all_req = true; }
                        let copy_last_btn = egui::Button::new(egui::RichText::new("Copy Last 40").strong().color(egui::Color32::WHITE))
                            .fill(egui::Color32::from_rgb(16, 125, 78));
                        if ui.add(copy_last_btn).clicked() { copy_last_req = true; }
                    });
                });

                if clear_req {
                    guard.clear();
                }

                let flt = guard.filter.trim().to_ascii_lowercase();
                let list: Vec<String> = if flt.is_empty() {
                    guard.lines.iter().cloned().collect()
                } else {
                    guard.lines.iter().filter(|l| l.to_ascii_lowercase().contains(&flt)).cloned().collect()
                };
                let all = guard.full_text();
                (guard.auto_scroll, flt, list, guard.total_lines, all)
            };

            if copy_all_req && runtime::copy_to_clipboard(&full_text) {
                ui.ctx().data_mut(|d| d.insert_temp(copy_id, now));
            }
            if copy_last_req {
                let start = filtered_lines.len().saturating_sub(40);
                let tail = filtered_lines[start..].join("\n");
                if runtime::copy_to_clipboard(&tail) {
                    ui.ctx().data_mut(|d| d.insert_temp(copy_id, now));
                }
            }
            if notepad_req {
                let fname = if is_llama { "llama_console.txt" } else { "owu_console.txt" };
                runtime::open_text_in_notepad(fname, &full_text);
            }
            if hide_req {
                open_flag.store(false, Ordering::Relaxed);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                return;
            }

            let shown_start = filtered_lines.len().saturating_sub(600);
            let display_text = filtered_lines[shown_start..].join("\n");

            ui.add_space(4.0);
            ui.add(egui::Label::new(
                egui::RichText::new(format!(
                    "Showing last {} of {} lines (total: {}){} — Closing this window keeps the server running.",
                    filtered_lines.len() - shown_start,
                    filtered_lines.len(),
                    total_lines,
                    if filter_lower.is_empty() { "" } else { " [filtered]" }
                ))
                .size(11.0)
                .color(egui::Color32::from_rgb(138, 146, 162)),
            ).selectable(false));
            ui.add_space(4.0);

            egui::Frame::new()
                .fill(egui::Color32::from_rgb(8, 9, 13))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(38, 42, 56)))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt((vp_id, "log_scroll"))
                        .auto_shrink([false, false])
                        .stick_to_bottom(auto_scroll)
                        .show(ui, |ui| {
                            let mut view_slice: &str = if display_text.is_empty() {
                                "Waiting for server output... Start a Routing Set or Model Profile from AI Control Center."
                            } else {
                                display_text.as_str()
                            };
                            ui.add(
                                egui::TextEdit::multiline(&mut view_slice)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(26),
                            );
                        });
                });
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn render_spec_inspector(
    ui: &mut egui::Ui,
    spec: &mut ModelSpec,
    edit_mode: bool,
    show_hints: bool,
    pal: UiPalette,
    models_dir: &str,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    port_val: &mut u16,
    id_salt: &str,
    trigger_save_msg: &mut Option<String>,
) {
    let c_vram = egui::Color32::from_rgb(96, 165, 250);
    let c_speed = egui::Color32::from_rgb(52, 211, 153);
    let c_qual = egui::Color32::from_rgb(251, 191, 36);
    let c_ram = egui::Color32::from_rgb(192, 132, 252);

    if edit_mode {
        ui.horizontal_wrapped(|ui| {
            ui.label("GGUF File:");
            let prev_model = spec.model_rel_path.clone();
            egui::ComboBox::from_id_salt((id_salt, "gguf_combo"))
                .width(305.0)
                .selected_text(&spec.model_rel_path)
                .show_ui(ui, |ui| {
                    for m in scanned_models.iter() {
                        ui.selectable_value(&mut spec.model_rel_path, m.clone(), m);
                    }
                });
            if spec.model_rel_path != prev_model {
                *trigger_save_msg = Some(format!("Updated GGUF file to '{}'.", spec.model_rel_path));
            }
            if ui.button("Rescan").clicked() {
                *scanned_models = scan_gguf_models(models_dir);
                gguf_cache.clear();
            }
            if ui.button("Pick...").clicked() {
                if let Some(p) = FileDialog::new()
                    .set_directory(models_dir)
                    .add_filter("GGUF", &["gguf"])
                    .pick_file()
                {
                    spec.model_rel_path = p
                        .strip_prefix(models_dir)
                        .map(|r| r.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.to_string_lossy().to_string());
                    gguf_cache.remove(&format!("{models_dir}::{}", spec.model_rel_path));
                    *trigger_save_msg = Some(format!("Linked GGUF '{}'.", spec.model_rel_path));
                }
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut spec.use_alias, "OWU Alias (--alias):")
                .on_hover_text("Sets the model ID shown in Open WebUI & llama-swap routing.");
            pasteable_singleline(ui, &mut spec.alias, 195.0, "");
            ui.label("Direct Port:");
            ui.add(egui::DragValue::new(port_val).range(1024..=65535))
                .on_hover_text("Local TCP port for direct single-profile launch (Routing Sets use the global API port).");
        });
    } else {
        ui.label(egui::RichText::new(format!("GGUF: {}", spec.model_rel_path)).strong());
    }

    // CACHED GGUF HEADER PROBE
    let cache_key = format!("{models_dir}::{}", spec.model_rel_path);
    let ginfo = gguf_cache
        .entry(cache_key)
        .or_insert_with(|| probe_gguf_header(models_dir, &spec.model_rel_path))
        .clone();

    if ginfo.valid {
        let layers_txt = ginfo.block_count.map(|b| format!("{b} layers")).unwrap_or_default();
        let ctx_txt = ginfo.context_length.map(|c| format!("native {c}")).unwrap_or_default();
        let exp_txt = ginfo.expert_count.map(|e| format!(" | {e} MoE experts")).unwrap_or_default();
        let hdr_col = if edit_mode { pal.edit_hl } else { pal.accent };
        ui.add(egui::Label::new(
            egui::RichText::new(format!(
                "GGUF Info (Cached): {:.1} GB | Arch: {} | {} | {}{}",
                ginfo.file_size_gb, ginfo.arch, layers_txt, ctx_txt, exp_txt
            ))
            .size(11.0)
            .color(hdr_col),
        ).selectable(false));
    }

    ui.add_space(4.0);

    // 1. HARDWARE & VRAM SPLIT
    egui::CollapsingHeader::new("1. Hardware, VRAM Split & MoE Cache").default_open(true).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let ngl_tip = "GPU Layers (-ngl): How many transformer blocks sit in 8GB VRAM.\nUP: Faster tok/s, uses more VRAM.\nDOWN: Frees VRAM for bigger context.";
            ui.label("GPU Layers (-ngl):").on_hover_text(ngl_tip);
            egui::ComboBox::from_id_salt((id_salt, "ngl")).selected_text(&spec.ngl_mode).show_ui(ui, |ui| {
                ui.selectable_value(&mut spec.ngl_mode, "auto".to_string(), "auto");
                ui.selectable_value(&mut spec.ngl_mode, "custom".to_string(), "custom");
            });
            if spec.ngl_mode == "custom" {
                ui.add(egui::DragValue::new(&mut spec.ngl_layers).range(0..=999)).on_hover_text(ngl_tip);
            }
            ui.separator();
            let fit_tip = "Auto-Fit (-fit): Automatic VRAM balancer.\nON: Protects against OOM.\nOFF: Strict mode — obeys exact -c and -ngl.";
            ui.label("Auto-Fit (-fit):").on_hover_text(fit_tip);
            egui::ComboBox::from_id_salt((id_salt, "fit")).selected_text(&spec.fit_mode).show_ui(ui, |ui| {
                ui.selectable_value(&mut spec.fit_mode, "default".to_string(), "default");
                ui.selectable_value(&mut spec.fit_mode, "on".to_string(), "on (Protect VRAM)");
                ui.selectable_value(&mut spec.fit_mode, "off".to_string(), "off (Strict)");
            });
            if spec.fit_mode == "on" {
                ui.label("Margin (MiB):").on_hover_text(fit_tip);
                ui.add(egui::DragValue::new(&mut spec.fit_target_mib).range(256..=4096).speed(64)).on_hover_text(fit_tip);
            }
        });
        plain_help_row(ui, show_hints, pal, &[("VRAM", c_vram), ("Speed", c_speed)], "GPU Layers (-ngl): UP = faster tok/s + more VRAM; DOWN = less VRAM + slower RAM compute. Auto-Fit OFF locks exact 256k context.");

        ui.horizontal_wrapped(|ui| {
            let moe_tip = "MoE CPU Offload: Keeps attention/shared layers on GPU while moving routed MoE experts to system RAM.";
            pill_switch(ui, &mut spec.moe_enabled, "MoE Offload: ON", "MoE Offload: OFF", edit_mode, pal, moe_tip);
            if spec.moe_enabled {
                pill_switch(ui, &mut spec.all_moe_cpu, "All CPU (-cmoe)", "Layer Count", edit_mode, pal, "All MoE on CPU (-cmoe) vs exact --n-cpu-moe count.");
                if !spec.all_moe_cpu {
                    ui.label("CPU Layers:").on_hover_text(moe_tip);
                    ui.add(egui::DragValue::new(&mut spec.n_cpu_moe).range(1..=99)).on_hover_text(moe_tip);
                }
                let cache_tip = "MoE Expert Cache (--moe-cache-mib): Dynamic VRAM cache for hottest MoE experts (0 = off).";
                ui.label("Cache (MiB):").on_hover_text(cache_tip);
                ui.add(egui::DragValue::new(&mut spec.moe_cache_mib).range(0..=6144).speed(64)).on_hover_text(cache_tip);

                if spec.moe_cache_mib > 0 {
                    let share_tip = "MoE CPU Share (--moe-cache-cpu-share): Fraction of cache misses computed by CPU instead of PCIe upload (0.00 to 1.00).";
                    ui.label("CPU Share:").on_hover_text(share_tip);
                    ui.add(egui::DragValue::new(&mut spec.moe_cache_cpu_share).range(0.0..=1.0).speed(0.05).fixed_decimals(2)).on_hover_text(share_tip);
                }
            }
        });
        plain_help_row(ui, show_hints, pal, &[("VRAM", c_vram), ("Speed", c_speed)], "MoE Offload: --n-cpu-moe UP = more VRAM free (offloads MoE layers to RAM). Expert Cache MiB UP = higher hit rate until VRAM limit.");

        ui.horizontal_wrapped(|ui| {
            ui.label("Dense CPU FFN (-ncffn):");
            ui.add(egui::DragValue::new(&mut spec.n_cpu_ffn).range(0..=99));
            ui.separator();
            pill_switch(ui, &mut spec.disable_cuda_pinned, "No CUDA Pinned: ON", "No CUDA Pinned: OFF", edit_mode, pal, "Sets GGML_CUDA_NO_PINNED=1 for this server process only.");
            ui.separator();
            pill_switch(ui, &mut spec.use_rpc, "RPC: ON", "RPC: OFF", edit_mode, pal, "Pools remote GPU VRAM over LAN (--rpc). Keep on its own sub-profile so daily sets work when the remote node is off.");
            if spec.use_rpc {
                pasteable_singleline(ui, &mut spec.rpc_endpoint, 165.0, "192.168.x.x:50052");
            }
        });
        plain_help_row(ui, show_hints, pal, &[("VRAM", c_vram), ("RAM", c_ram)], "Dense FFN UP = frees VRAM on dense models. No CUDA Pinned ON = prevents Shared GPU spikes on Windows. RPC ON = pools remote GPU VRAM over LAN.");
    });

    // 2. CONTEXT & KV CACHE COMPRESSION
    egui::CollapsingHeader::new("2. Context & Memory Compression (KV Cache)").default_open(true).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let ctx_tip = "Context Size (-c): Total conversation memory in tokens. Also replaces {ctx} in custom engines like RustingStrata.";
            ui.label("Context (-c):").on_hover_text(ctx_tip);
            egui::ComboBox::from_id_salt((id_salt, "ctx")).selected_text(format!("Preset ({})", spec.ctx_size)).width(120.0).show_ui(ui, |ui| {
                for c in [8192, 16384, 32768, 65536, 131072, 262144] {
                    ui.selectable_value(&mut spec.ctx_size, c, c.to_string());
                }
            });
            ui.add(egui::DragValue::new(&mut spec.ctx_size).range(2048..=1_048_576).speed(1024)).on_hover_text(ctx_tip);

            let kv_tip = "KV Cache Quantization (-ctk / -ctv): Higher (f16/q8_0) = sharper recall + more VRAM; Lower (q4_0/turbo3) = big VRAM savings.";
            ui.label("KV K (-ctk):").on_hover_text(kv_tip);
            egui::ComboBox::from_id_salt((id_salt, "ctk")).selected_text(&spec.cache_type_k).width(78.0).show_ui(ui, |ui| {
                for k in ["f16", "bf16", "f8_e4m3", "q8_0", "q4_0", "iq4_nl", "turbo4", "turbo3", "turbo2"] {
                    ui.selectable_value(&mut spec.cache_type_k, k.to_string(), k);
                }
            });
            ui.label("KV V (-ctv):").on_hover_text(kv_tip);
            egui::ComboBox::from_id_salt((id_salt, "ctv")).selected_text(&spec.cache_type_v).width(78.0).show_ui(ui, |ui| {
                for v in ["f16", "bf16", "f8_e4m3", "q8_0", "q4_0", "iq4_nl", "turbo4", "turbo3", "turbo2"] {
                    ui.selectable_value(&mut spec.cache_type_v, v.to_string(), v);
                }
            });
        });
        plain_help_row(ui, show_hints, pal, &[("VRAM", c_vram), ("Speed", c_speed), ("Quality", c_qual)], "Context UP = longer memory + higher VRAM; DOWN = frees VRAM for speed. Use turbo3 on TurboQuant fork for long dense/Gemma 4 context.");

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut spec.no_kv_offload, "CPU KV (-nkvo)");
            ui.checkbox(&mut spec.kv_unified, "Unified KV (-kvu)");
            ui.checkbox(&mut spec.swa_full, "Full SWA (--swa-full)");
            ui.label("Checkpoints:");
            ui.add(egui::DragValue::new(&mut spec.ctx_checkpoints).range(0..=64));
            ui.label("Cache RAM (MiB):");
            ui.add(egui::DragValue::new(&mut spec.cache_ram_mib).range(-1..=16384).speed(128));
            ui.label("Parallel:");
            ui.add(egui::DragValue::new(&mut spec.parallel_slots).range(1..=8));
        });
    });

    // 3. THINKING, SPECULATIVE/MTP & ENGINE THROUGHPUT
    egui::CollapsingHeader::new("3. Thinking, Speculative/MTP & Engine Throughput").default_open(true).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if edit_mode {
                ui.label("Reasoning Family:");
                egui::ComboBox::from_id_salt((id_salt, "rfam")).selected_text(&spec.reasoning_family).show_ui(ui, |ui| {
                    ui.selectable_value(&mut spec.reasoning_family, "none".to_string(), "none (Instruct)");
                    ui.selectable_value(&mut spec.reasoning_family, "binary".to_string(), "binary (on/off)");
                    ui.selectable_value(&mut spec.reasoning_family, "effort3".to_string(), "effort3 (low/med/xhigh)");
                });
            }

            match spec.reasoning_family.as_str() {
                "effort3" => {
                    ui.label("Effort:");
                    egui::ComboBox::from_id_salt((id_salt, "eff3")).selected_text(&spec.reasoning_mode).show_ui(ui, |ui| {
                        ui.selectable_value(&mut spec.reasoning_mode, "xhigh".to_string(), "xhigh (Deepest)");
                        ui.selectable_value(&mut spec.reasoning_mode, "medium".to_string(), "medium (Balanced)");
                        ui.selectable_value(&mut spec.reasoning_mode, "low".to_string(), "low (Fast)");
                        ui.selectable_value(&mut spec.reasoning_mode, "off".to_string(), "off (No Thinking)");
                    });
                }
                "binary" => {
                    ui.label("Thinking:");
                    egui::ComboBox::from_id_salt((id_salt, "bin")).selected_text(&spec.reasoning_mode).show_ui(ui, |ui| {
                        ui.selectable_value(&mut spec.reasoning_mode, "on".to_string(), "on (Step-by-Step Think)");
                        ui.selectable_value(&mut spec.reasoning_mode, "off".to_string(), "off (Instant Direct Reply)");
                    });
                }
                _ => {
                    ui.label("Reasoning: Pure Instruct");
                }
            }

            if spec.reasoning_family != "none" && spec.reasoning_mode != "off" {
                ui.label("Format:");
                egui::ComboBox::from_id_salt((id_salt, "rfmt")).selected_text(&spec.reasoning_format).width(85.0).show_ui(ui, |ui| {
                    for rf in ["deepseek", "default", "none"] {
                        ui.selectable_value(&mut spec.reasoning_format, rf.to_string(), rf);
                    }
                });
                ui.checkbox(&mut spec.reasoning_preserve, "Preserve Think");
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.label("Speculative (--spec-type):");
            let mut cur_spec = spec.effective_spec_type().to_string();
            let prev_spec = cur_spec.clone();
            egui::ComboBox::from_id_salt((id_salt, "spectype")).selected_text(&cur_spec).width(115.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut cur_spec, "off".to_string(), "off");
                ui.selectable_value(&mut cur_spec, "draft-mtp".to_string(), "draft-mtp (MTP)");
                ui.selectable_value(&mut cur_spec, "ngram-simple".to_string(), "ngram-simple");
                ui.selectable_value(&mut cur_spec, "ngram-mod".to_string(), "ngram-mod");
            });
            if cur_spec != prev_spec {
                spec.set_spec_type(&cur_spec);
            }

            if spec.effective_spec_type() != "off" {
                ui.label("Draft Max:");
                ui.add(egui::DragValue::new(&mut spec.mtp_draft_max).range(1..=16));
                ui.label("Min-P:");
                ui.add(egui::DragValue::new(&mut spec.mtp_draft_p_min).range(0.0..=0.99).speed(0.05).fixed_decimals(2));
            }
        });

        if spec.effective_spec_type() == "draft-mtp" && edit_mode {
            ui.horizontal(|ui| {
                ui.label("External Draft Head (--model-draft):");
                pasteable_singleline(ui, &mut spec.mtp_draft_model_rel, 240.0, "blank if built-in");
            });
        }

        ui.horizontal_wrapped(|ui| {
            ui.label("Flash Attn (-fa):");
            egui::ComboBox::from_id_salt((id_salt, "fa")).selected_text(&spec.flash_attn).width(62.0).show_ui(ui, |ui| {
                for f in ["on", "off", "auto"] { ui.selectable_value(&mut spec.flash_attn, f.to_string(), f); }
            });
            ui.label("Load Mode (-lm):");
            egui::ComboBox::from_id_salt((id_salt, "lm")).selected_text(&spec.load_mode).width(68.0).show_ui(ui, |ui| {
                for m in ["auto", "none", "mmap", "mlock", "dio"] { ui.selectable_value(&mut spec.load_mode, m.to_string(), m); }
            });
            ui.label("Batch / uBatch:");
            ui.add(egui::DragValue::new(&mut spec.batch_size).range(128..=8192).speed(128));
            ui.add(egui::DragValue::new(&mut spec.ubatch_size).range(64..=4096).speed(64));
            ui.label("Threads (-t / -tb):");
            ui.add(egui::DragValue::new(&mut spec.threads).range(1..=32));
            ui.add(egui::DragValue::new(&mut spec.threads_batch).range(0..=32));
        });

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut spec.jinja, "--jinja");
            ui.checkbox(&mut spec.enable_metrics, "--metrics");
            ui.label("Verbosity (-lv):");
            ui.add(egui::DragValue::new(&mut spec.verbosity).range(-1..=4));
            ui.label("Timeout (s):");
            ui.add(egui::DragValue::new(&mut spec.server_timeout).range(0..=3600).speed(30));
        });

        ui.horizontal(|ui| {
            ui.label("Extra / Custom Flags:")
                .on_hover_text("For llama_cpp: appended at the end. For custom engines (e.g. RustingStrata): used verbatim with {model}, {ctx}, {host}, {port}, {addr}.");
            pasteable_singleline(ui, &mut spec.extra_args, 310.0, "Extra CLI flags or full custom engine command...");
            if ui.button("Paste").clicked() {
                if let Some(clip) = runtime::read_from_clipboard() {
                    let cleaned = clip.replace(['\r', '\n'], " ");
                    let trimmed = cleaned.trim();
                    if !trimmed.is_empty() {
                        if !spec.extra_args.trim().is_empty() && !spec.extra_args.ends_with(' ') {
                            spec.extra_args.push(' ');
                        }
                        spec.extra_args.push_str(trimmed);
                    }
                }
            }
            if !spec.extra_args.is_empty() && ui.button("Clear").clicked() {
                spec.extra_args.clear();
            }
        });
        plain_help_row(ui, show_hints, pal, &[("Speed", c_speed), ("Quality", c_qual)], "For custom engines (RustingStrata), put the full flags in Extra / Custom Flags using {model}, {ctx}, and {addr}.");
    });

    // 4. OPTIONAL SAMPLERS & DRY
    egui::CollapsingHeader::new("4. Samplers & DRY (Optional - Open WebUI Overrides When Set)").default_open(false).show(ui, |ui| {
        plain_help_row(ui, true, pal, &[("Quality", c_qual)], "Temp UP = creative, DOWN = factual. Pres/Rep Pen UP = avoids repetition (keep Pres=0 in Thinking mode). DRY breaks phrase loops in long stories.");
        ui.horizontal_wrapped(|ui| {
            pill_switch(ui, &mut spec.use_custom_sampling, "Samplers: ON", "Samplers: Default", edit_mode, pal, "ON sends explicit sampler flags; OFF lets server/OWU decide.");
            if spec.use_custom_sampling {
                ui.label("Temp:");
                ui.add(egui::DragValue::new(&mut spec.temp).speed(0.05).range(0.0..=2.0));
                ui.label("Min-P:");
                ui.add(egui::DragValue::new(&mut spec.min_p).speed(0.01).range(0.0..=1.0));
                ui.label("Top-P:");
                ui.add(egui::DragValue::new(&mut spec.top_p).speed(0.01).range(0.0..=1.0));
                ui.label("Top-K:");
                ui.add(egui::DragValue::new(&mut spec.top_k).range(0..=200));
            }
        });
        if spec.use_custom_sampling {
            ui.horizontal_wrapped(|ui| {
                ui.label("Pres Pen:");
                ui.add(egui::DragValue::new(&mut spec.presence_penalty).speed(0.1).range(0.0..=2.0));
                ui.label("Rep Pen:");
                ui.add(egui::DragValue::new(&mut spec.repeat_penalty).speed(0.05).range(0.5..=2.0));
                ui.label("DRY Mult:");
                ui.add(egui::DragValue::new(&mut spec.dry_multiplier).speed(0.05).range(0.0..=3.0));
                ui.label("DRY Base:");
                ui.add(egui::DragValue::new(&mut spec.dry_base).speed(0.05).range(1.0..=4.0));
                ui.label("DRY Len:");
                ui.add(egui::DragValue::new(&mut spec.dry_allowed_length).range(1..=10));
            });
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn render_advanced_inspector(
    ui: &mut egui::Ui,
    prof: &mut ModelProfile,
    edit_mode: bool,
    show_hints: bool,
    pal: UiPalette,
    models_dir: &str,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    trigger_save_msg: &mut Option<String>,
) {
    let c_qual = egui::Color32::from_rgb(251, 191, 36);
    let c_vram = egui::Color32::from_rgb(96, 165, 250);

    if edit_mode {
        ui.horizontal_wrapped(|ui| {
            ui.label("GGUF File:");
            let prev = prof.single.model_rel_path.clone();
            egui::ComboBox::from_id_salt("adv_gguf_combo")
                .width(265.0)
                .selected_text(&prof.single.model_rel_path)
                .show_ui(ui, |ui| {
                    for m in scanned_models.iter() {
                        ui.selectable_value(&mut prof.single.model_rel_path, m.clone(), m);
                    }
                });
            if prof.single.model_rel_path != prev {
                *trigger_save_msg = Some(format!("Updated GGUF file to '{}'.", prof.single.model_rel_path));
            }
            if ui.button("Rescan").clicked() {
                *scanned_models = scan_gguf_models(models_dir);
                gguf_cache.clear();
            }
            if ui.button("Pick...").clicked() {
                if let Some(p) = FileDialog::new()
                    .set_directory(models_dir)
                    .add_filter("GGUF", &["gguf"])
                    .pick_file()
                {
                    prof.single.model_rel_path = p
                        .strip_prefix(models_dir)
                        .map(|r| r.to_string_lossy().to_string())
                        .unwrap_or_else(|_| p.to_string_lossy().to_string());
                    gguf_cache.remove(&format!("{models_dir}::{}", prof.single.model_rel_path));
                    *trigger_save_msg = Some(format!("Linked GGUF '{}'.", prof.single.model_rel_path));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut prof.single.use_alias, "OWU Alias:");
            pasteable_singleline(ui, &mut prof.single.alias, 180.0, "");
            ui.label("Context ({ctx}):");
            ui.add(egui::DragValue::new(&mut prof.single.ctx_size).range(2048..=1_048_576).speed(1024));
            ui.label("Direct Port:");
            ui.add(egui::DragValue::new(&mut prof.port).range(1024..=65535));
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("GGUF: {}", prof.single.model_rel_path)).strong());
            ui.separator();
            ui.label("Ctx ({ctx}):");
            ui.add(egui::DragValue::new(&mut prof.single.ctx_size).range(2048..=1_048_576).speed(1024));
        });
    }

    let cache_key = format!("{models_dir}::{}", prof.single.model_rel_path);
    let ginfo = gguf_cache
        .entry(cache_key)
        .or_insert_with(|| probe_gguf_header(models_dir, &prof.single.model_rel_path))
        .clone();
    if ginfo.valid {
        let layers_txt = ginfo.block_count.map(|b| format!("{b} layers")).unwrap_or_default();
        let ctx_txt = ginfo.context_length.map(|c| format!("native {c}")).unwrap_or_default();
        let exp_txt = ginfo.expert_count.map(|e| format!(" | {e} MoE experts")).unwrap_or_default();
        let hdr_col = if edit_mode { pal.edit_hl } else { pal.accent };
        ui.add(egui::Label::new(
            egui::RichText::new(format!(
                "GGUF Info (Cached): {:.1} GB | Arch: {} | {} | {}{}",
                ginfo.file_size_gb, ginfo.arch, layers_txt, ctx_txt, exp_txt
            ))
            .size(11.0)
            .color(hdr_col),
        ).selectable(false));
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Manual CLI Flags (Raw Override):").strong().color(pal.text));
        if ui.button("Pull From Simple").on_hover_text("Overwrite box with flags generated from Simple Mode controls.").clicked() {
            prof.manual_flags = prof.generate_manual_template();
        }
        if ui.button("Paste").on_hover_text("Paste flags from clipboard.").clicked() {
            if let Some(clip) = runtime::read_from_clipboard() {
                let cleaned = clip.replace(['\r', '\n'], " ");
                let trimmed = cleaned.trim();
                if !trimmed.is_empty() {
                    if !prof.manual_flags.trim().is_empty() && !prof.manual_flags.ends_with(' ') {
                        prof.manual_flags.push(' ');
                    }
                    prof.manual_flags.push_str(trimmed);
                }
            }
        }
        if !prof.manual_flags.is_empty() && ui.button("Clear").clicked() {
            prof.manual_flags.clear();
        }
        pill_switch(ui, &mut prof.single.disable_cuda_pinned, "No CUDA Pinned: ON", "No CUDA Pinned: OFF", edit_mode, pal, "Sets GGML_CUDA_NO_PINNED=1 for this run.");
    });

    plain_help_row(
        ui,
        show_hints,
        pal,
        &[("Placeholders", c_vram)],
        "Use {model}, {ctx}, {host}, {port}, {addr}. Use {port} or {addr} so llama-swap Routing Sets work without port conflicts. Unsaved edits run as temporary tests until you click Save Changes.",
    );

    ui.add_space(4.0);
    egui::Frame::new()
        .fill(pal.code_bg)
        .stroke(egui::Stroke::new(1.0, if edit_mode { pal.edit_hl } else { pal.card_border }))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut prof.manual_flags)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("Type any CLI flags here... e.g. -m {model} -c {ctx} -ngl 99 -fa on --host {host} --port {port}")
                    .desired_width(f32::INFINITY)
                    .desired_rows(9),
            );
        });

    ui.add_space(8.0);
    egui::CollapsingHeader::new("Temperature, Samplers & DRY (Auto-appended if not typed above)")
        .default_open(true)
        .show(ui, |ui| {
            plain_help_row(ui, show_hints, pal, &[("Samplers", c_qual)], "If Samplers is ON and you did not manually type --temp above, these values are appended automatically.");
            ui.horizontal_wrapped(|ui| {
                pill_switch(ui, &mut prof.single.use_custom_sampling, "Samplers: ON", "Samplers: Default", edit_mode, pal, "ON appends sampler flags below; OFF uses engine/OWU defaults or whatever you typed manually.");
                if prof.single.use_custom_sampling {
                    ui.label("Temp:");
                    ui.add(egui::DragValue::new(&mut prof.single.temp).speed(0.05).range(0.0..=2.0));
                    ui.label("Min-P:");
                    ui.add(egui::DragValue::new(&mut prof.single.min_p).speed(0.01).range(0.0..=1.0));
                    ui.label("Top-P:");
                    ui.add(egui::DragValue::new(&mut prof.single.top_p).speed(0.01).range(0.0..=1.0));
                    ui.label("Top-K:");
                    ui.add(egui::DragValue::new(&mut prof.single.top_k).range(0..=200));
                }
            });
            if prof.single.use_custom_sampling {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Pres Pen:");
                    ui.add(egui::DragValue::new(&mut prof.single.presence_penalty).speed(0.1).range(0.0..=2.0));
                    ui.label("Rep Pen:");
                    ui.add(egui::DragValue::new(&mut prof.single.repeat_penalty).speed(0.05).range(0.5..=2.0));
                    ui.label("DRY Mult:");
                    ui.add(egui::DragValue::new(&mut prof.single.dry_multiplier).speed(0.05).range(0.0..=3.0));
                    ui.label("DRY Base:");
                    ui.add(egui::DragValue::new(&mut prof.single.dry_base).speed(0.05).range(1.0..=4.0));
                    ui.label("DRY Len:");
                    ui.add(egui::DragValue::new(&mut prof.single.dry_allowed_length).range(1..=10));
                });
            }
        });
}

struct ControlCenterApp {
    patch: AppPatch,
    cfg: StackConfig,
    saved_cfg: StackConfig,
    scanned_models: Vec<String>,
    gguf_cache: HashMap<String, GgufInfo>,
    help_map: HelpMap,

    new_set_name: String,
    new_group_name: String,
    new_profile_name: String,
    new_engine_label: String,
    new_engine_kind: String,
    new_engine_dir: String,
    new_engine_exe: String,
    new_engine_args: String,
    new_engine_color: [u8; 3],

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
        if !from_disk.is_empty() { from_disk } else if let Some(emb) = embedded { parse_help_markdown_str(emb) } else { from_disk }
    }

    fn new(ctx: egui::Context, patch: AppPatch) -> Self {
        let cfg = StackConfig::load_with_hooks(patch.default_config_fn, patch.legacy_migrator_fn, patch.post_load_fn);
        let saved_cfg = cfg.clone();
        let scanned_models = scan_gguf_models(&cfg.models_dir);
        let help_map = Self::load_help_with_fallback(&cfg.help_md_path, patch.embedded_help_md);
        let init_exes = collect_backend_exes(&cfg);
        let init_fe_exe = cfg.effective_frontend_exe_name();
        let init_comp_ports: Vec<u16> = cfg.companion_services.iter().map(|s| s.check_port).collect();
        let initial = poll_processes_full(&init_exes, &init_fe_exe, &init_comp_ports);
        let loaded_label = if initial.llama { Some(format!("Active Engine (:{})", cfg.server_port)) } else { None };

        let shared_status = Arc::new(Mutex::new(initial));
        let shared_exes = Arc::new(Mutex::new(init_exes));
        let shared_fe_exe = Arc::new(Mutex::new(init_fe_exe));
        let shared_comp_ports = Arc::new(Mutex::new(init_comp_ports));
        let (bg_status, bg_exes, bg_fe, bg_ports, bg_ctx) = (
            Arc::clone(&shared_status), Arc::clone(&shared_exes),
            Arc::clone(&shared_fe_exe), Arc::clone(&shared_comp_ports), ctx.clone(),
        );

        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(2500));
            let exes = bg_exes.lock().map(|g| g.clone()).unwrap_or_else(|_| vec![platform::default_exe_name("llama-swap"), platform::default_exe_name("llama-server")]);
            let fe = bg_fe.lock().map(|g| g.clone()).unwrap_or_default();
            let ports = bg_ports.lock().map(|g| g.clone()).unwrap_or_default();
            let latest = poll_processes_full(&exes, &fe, &ports);
            if let Ok(mut guard) = bg_status.lock() { *guard = latest; }
            bg_ctx.request_repaint_of(egui::ViewportId::ROOT);
        });

        let sig = (cfg.theme_mode, cfg.accent_rgb, false);
        let ready_msg = format!("{} Ready — pick a Routing Set on top, or browse Model Families -> Sub-Profiles below.", patch.effective_version());
        Self {
            patch, cfg, saved_cfg, scanned_models, gguf_cache: HashMap::new(), help_map,
            new_set_name: String::new(), new_group_name: String::new(), new_profile_name: String::new(),
            new_engine_label: String::new(), new_engine_kind: "llama_cpp".to_string(),
            new_engine_dir: String::new(), new_engine_exe: platform::default_exe_name("llama-server"), new_engine_args: String::new(), new_engine_color: ENGINE_COLOR_PALETTE[1],
            loaded_label, shared_status, shared_exes, shared_fe_exe, shared_comp_ports,
            llama_console: Arc::new(Mutex::new(ConsoleBuffer::default())),
            owu_console: Arc::new(Mutex::new(ConsoleBuffer::default())),
            show_llama_console: Arc::new(AtomicBool::new(false)),
            show_owu_console: Arc::new(AtomicBool::new(false)),
            error_logs: Arc::new(Mutex::new(Vec::new())),
            show_error_log: false, show_plain_hints: true, show_theme_bar: false,
            right_drawer: None, code_scope: CodePreviewScope::RoutingSet,
            live_code_select_mode: false, copied_code_at: -10.0, edit_mode: false,
            applied_theme_sig: sig, status_msg: ready_msg,
        }
    }

    fn sync_backend_exes(&self) {
        if let Ok(mut g) = self.shared_exes.lock() { *g = collect_backend_exes(&self.cfg); }
        if let Ok(mut g) = self.shared_fe_exe.lock() { *g = self.cfg.effective_frontend_exe_name(); }
        if let Ok(mut g) = self.shared_comp_ports.lock() { *g = self.cfg.companion_services.iter().map(|s| s.check_port).collect(); }
    }

    fn reload_help(&mut self) {
        self.help_map = Self::load_help_with_fallback(&self.cfg.help_md_path, self.patch.embedded_help_md);
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
            apply_app_theme(ui.ctx(), self.cfg.theme_mode, self.cfg.accent_rgb, self.edit_mode);
            self.applied_theme_sig = want_sig;
        }

        let pal = palette_for(self.cfg.theme_mode, self.cfg.accent_rgb, self.edit_mode);

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

        // RIGHT-SIDE TABBED DRAWER (HELP | CODE / YAML) — HIDDEN UNLESS TOGGLED ON
        if let Some(active_tab) = self.right_drawer {
            let mut reload_help_req = false;
            let mut close_drawer_req = false;

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
                            egui::RichText::new("Help")
                                .strong()
                                .color(if help_sel { pal.accent_text } else { pal.btn_idle_text }),
                        )
                        .fill(if help_sel { pal.accent } else { pal.btn_idle });
                        if ui.add(h_btn).clicked() {
                            self.right_drawer = Some(RightDrawerTab::Help);
                        }
}

                        let c_btn = egui::Button::new(
                            egui::RichText::new("Code / YAML")
                                .strong()
                                .color(if code_sel { pal.accent_text } else { pal.btn_idle_text }),
                        )
                        .fill(if code_sel { pal.accent } else { pal.btn_idle });
                        if ui.add(c_btn).clicked() {
                            self.right_drawer = Some(RightDrawerTab::CodeYaml);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("X").on_hover_text("Close side panel").clicked() {
                                close_drawer_req = true;
                            }
                            if active_tab == RightDrawerTab::Help {
                                if ui.button("Edit").on_hover_text("Open external help.md in Notepad").clicked() {
                                    runtime::open_existing_file_in_notepad(&StackConfig::resolve_config_relative(&self.cfg.help_md_path));
                                }
                                if ui.button("Reload").on_hover_text("Reload help.md from disk").clicked() {
                                    reload_help_req = true;
                                }
                            }
                        });
                    });
                    ui.separator();

                    let sel_pair: Option<(String, ModelProfile)> = self
                        .cfg
                        .groups
                        .get(self.cfg.selected_group_idx)
                        .and_then(|g| {
                            g.profiles
                                .get(self.cfg.selected_profile_idx)
                                .map(|p| (g.model_title.clone(), p.clone()))
                        });
                    let sel_set = self.cfg.routing_sets.get(self.cfg.selected_set_idx).cloned();

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

                                    ui.add(egui::Label::new(
                                        egui::RichText::new(format!("{g_title} — {}", prof.name))
                                            .size(14.0)
                                            .strong()
                                            .color(pal.text),
                                    ).selectable(false));
                                    ui.add(egui::Label::new(
                                        egui::RichText::new(format!(
                                            "@model: {hkey}  |  @engine: {eng_tag}{}",
                                            if hvar.is_empty() { String::new() } else { format!("  |  @variant: {hvar}") }
                                        ))
                                        .size(11.0)
                                        .color(pal.muted),
                                    ).selectable(false));
                                    ui.add_space(10.0);

                                    let gen_txt = self.help_map.get(&(hkey.clone(), "general".to_string(), String::new()));
                                    let eng_txt = self
                                        .help_map
                                        .get(&(hkey.clone(), eng_tag.to_string(), hvar.clone()))
                                        .or_else(|| self.help_map.get(&(hkey.clone(), eng_tag.to_string(), String::new())));
                                    let global_eng = self.help_map.get(&("*".to_string(), eng_tag.to_string(), String::new()));

                                    ui.label(egui::RichText::new("GENERAL MODEL NOTES").size(11.5).strong().color(pal.accent));
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
                                    let eng_col = engine_badge_color_for(eng_tag, &self.cfg.saved_engines);
                                    ui.label(
                                        egui::RichText::new(format!("{} ENGINE NOTES", eng_tag.to_ascii_uppercase()))
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
                                        ui.label(egui::RichText::new("GLOBAL ENGINE NOTES").size(11.0).strong().color(pal.muted));
                                        ui.add_space(4.0);
                                        ui.label(gtxt);
                                    }

                                    ui.add_space(14.0);
                                    ui.separator();
                                    ui.label(
                                        egui::RichText::new(format!("Source: {}", self.cfg.help_md_path))
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
                                        &mut self.code_scope,
                                        CodePreviewScope::RoutingSet,
                                        "Active Routing Set",
                                    );
                                    ui.selectable_value(
                                        &mut self.code_scope,
                                        CodePreviewScope::SelectedProfile,
                                        "Selected Profile CLI",
                                    );
                                });
                                ui.add_space(6.0);

                                let now = ui.ctx().input(|i| i.time);
                                let recently_copied = (now - self.copied_code_at) < 2.5;

                                match self.code_scope {
                                    CodePreviewScope::RoutingSet => {
                                        if let Some(set) = sel_set {
                                            let is_ini = set.mode == "llama_ini";
                                            let code = if is_ini {
                                                self.cfg.build_routing_set_ini(&set)
                                            } else {
                                                self.cfg.build_routing_set_yaml(&set)
                                            };
                                            let hdr = if is_ini {
                                                format!("models.ini — {}", set.name)
                                            } else {
                                                format!("llama-swap.yaml — {}", set.name)
                                            };

                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(egui::RichText::new(&hdr).strong().size(12.0).color(pal.accent));
                                                if recently_copied {
                                                    ui.label(egui::RichText::new("Copied!").strong().color(egui::Color32::from_rgb(52, 211, 153)));
                                                }
                                            });
                                            ui.horizontal(|ui| {
                                                let copy_btn = egui::Button::new(egui::RichText::new("Copy YAML/INI").strong().color(pal.accent_text)).fill(pal.accent);
                                                if ui.add(copy_btn).clicked() && runtime::copy_to_clipboard(&code) {
                                                    self.copied_code_at = now;
                                                    self.status_msg = "Copied Routing Set config to clipboard!".to_string();
                                                }
                                                if ui.button("Notepad").clicked() {
                                                    let fname = if is_ini { "models_preview.ini" } else { "llama_swap_preview.yaml" };
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
                                            let saved_prof = self.saved_cfg.find_profile(&prof.id).unwrap_or(&prof);
                                            let (diff_job, removed_items, change_count) =
                                                build_diff_layout_job(&prof, saved_prof, &self.cfg.models_dir);
                                            let full_code = prof.code_text(&self.cfg.models_dir);

                                            ui.horizontal_wrapped(|ui| {
                                                if change_count > 0 {
                                                    ui.label(
                                                        egui::RichText::new(format!("{change_count} unsaved flag changes (Green=Added, Amber=Modified)"))
                                                            .size(11.0)
                                                            .color(egui::Color32::from_rgb(251, 191, 36)),
                                                    );
                                                } else {
                                                    ui.label(egui::RichText::new("Matches saved config").size(11.0).color(pal.muted));
                                                }
                                                if recently_copied {
                                                    ui.label(egui::RichText::new("Copied!").strong().color(egui::Color32::from_rgb(52, 211, 153)));
                                                }
                                            });
                                            ui.horizontal_wrapped(|ui| {
                                                let copy_btn = egui::Button::new(egui::RichText::new("Copy CLI").strong().color(pal.accent_text)).fill(pal.accent);
                                                if ui.add(copy_btn).clicked() && runtime::copy_to_clipboard(&full_code) {
                                                    self.copied_code_at = now;
                                                    self.status_msg = "Copied profile CLI to clipboard!".to_string();
                                                }
                                                if ui.button("Notepad").clicked() {
                                                    runtime::open_text_in_notepad("live_flags.txt", &full_code);
                                                }
                                                let mode_lbl = if self.live_code_select_mode { "Show Color Diff" } else { "Select Text" };
                                                if ui.button(mode_lbl).clicked() {
                                                    self.live_code_select_mode = !self.live_code_select_mode;
                                                }
                                            });
                                            ui.add_space(6.0);

                                            egui::Frame::new()
                                                .fill(pal.code_bg)
                                                .stroke(egui::Stroke::new(1.0, pal.card_border))
                                                .corner_radius(egui::CornerRadius::same(8))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    if self.live_code_select_mode {
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
                                                            let rem_txt = format!("Removed: - {}", removed_items.join("   - "));
                                                            ui.add(egui::Label::new(
                                                                egui::RichText::new(rem_txt)
                                                                    .monospace()
                                                                    .size(11.0)
                                                                    .strikethrough()
                                                                    .color(egui::Color32::from_rgb(248, 113, 113)),
                                                            ).selectable(true));
                                                        }
                                                    }
                                                });
                                        }
                                    }
                                }
                            }
                        });
                });

            if reload_help_req {
                self.reload_help();
            }
            if close_drawer_req {
                self.right_drawer = None;
            }
        }

        // MAIN CENTRAL PANEL
        egui::CentralPanel::default().show(ui, |ui| {
            // TOP HEADER CARD
            themed_card(pal).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let title_str = if edit_mode {
                        format!("{} {}  [EDIT MODE]", effective_app_title(&self.cfg, &self.patch).to_uppercase(), self.patch.effective_version())
                    } else {
                        format!("{} {}", effective_app_title(&self.cfg, &self.patch).to_uppercase(), self.patch.effective_version())
                    };
                    let title_col = if edit_mode { pal.edit_hl } else { pal.text };
                    ui.add(egui::Label::new(egui::RichText::new(title_str).size(17.5).strong().color(title_col)).selectable(false));
                    ui.add_space(8.0);
                    let st_col = if edit_mode { pal.edit_hl } else { pal.accent };
                    ui.add(egui::Label::new(egui::RichText::new(&self.status_msg).size(12.0).color(st_col)).selectable(false));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let err_lbl = format!("Error Log ({err_count})");
                        let err_fill = if err_count > 0 { egui::Color32::from_rgb(165, 32, 38) } else { pal.btn_idle };
                        let err_txt = if err_count > 0 { egui::Color32::WHITE } else { pal.btn_idle_text };
                        if ui.add(egui::Button::new(egui::RichText::new(err_lbl).strong().color(err_txt)).fill(err_fill)).clicked() {
                            self.show_error_log = !self.show_error_log;
                        }

                        // Right-side tabbed drawer toggles (hidden by default)
                        let code_open = self.right_drawer == Some(RightDrawerTab::CodeYaml);
                        let code_btn = egui::Button::new(
                            egui::RichText::new("Code / YAML").strong().color(if code_open { pal.accent_text } else { pal.btn_idle_text }),
                        )
                        .fill(if code_open { pal.accent } else { pal.btn_idle });
                        if ui.add(code_btn).clicked() {
                            self.right_drawer = if code_open { None } else { Some(RightDrawerTab::CodeYaml) };
                        }

                        if cfg!(feature = "plus") {
let help_open = self.right_drawer == Some(RightDrawerTab::Help);
                        let help_btn = egui::Button::new(
                            egui::RichText::new("Help").strong().color(if help_open { pal.accent_text } else { pal.btn_idle_text }),
                        )
                        .fill(if help_open { pal.accent } else { pal.btn_idle });
                        if ui.add(help_btn).clicked() {
                            self.right_drawer = if help_open { None } else { Some(RightDrawerTab::Help) };
                        }
}

                        // Theme toggle (hidden by default)
                        let th_btn = egui::Button::new(
                            egui::RichText::new("Theme").strong().color(if self.show_theme_bar { pal.accent_text } else { pal.btn_idle_text }),
                        )
                        .fill(if self.show_theme_bar { pal.accent } else { pal.btn_idle });
                        if ui.add(th_btn).clicked() {
                            self.show_theme_bar = !self.show_theme_bar;
                        }

                        let hint_lbl = if self.show_plain_hints { "Hints: ON" } else { "Hints: OFF" };
                        let hint_fill = if self.show_plain_hints { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                        let hint_txt = if self.show_plain_hints { egui::Color32::WHITE } else { pal.btn_idle_text };
                        if ui.add(egui::Button::new(egui::RichText::new(hint_lbl).strong().color(hint_txt)).fill(hint_fill)).clicked() {
                            self.show_plain_hints = !self.show_plain_hints;
                        }

                        let (em_lbl, em_fill, em_txt) = if edit_mode {
                            ("EDIT MODE: ON", egui::Color32::WHITE, egui::Color32::BLACK)
                        } else {
                            ("Edit Mode: OFF", pal.btn_idle, pal.btn_idle_text)
                        };
                        let em_btn = egui::Button::new(egui::RichText::new(em_lbl).strong().color(em_txt))
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

                // COLLAPSIBLE THEME BAR (ONLY SHOWN WHEN THEME TOGGLED ON)
                if self.show_theme_bar {
                    ui.add_space(6.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("Surface:").size(12.0).color(pal.muted));
                        let mut theme_changed = false;
                        theme_changed |= ui.selectable_value(&mut self.cfg.theme_mode, ThemeMode::OledBlack, "OLED Black (#000)").changed();
                        theme_changed |= ui.selectable_value(&mut self.cfg.theme_mode, ThemeMode::Grey, "Grey (Mid)").changed();
                        theme_changed |= ui.selectable_value(&mut self.cfg.theme_mode, ThemeMode::Light, "Light").changed();

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
                    let llama_fill = if procs.llama { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                    let llama_txt = if procs.llama { egui::Color32::WHITE } else { pal.btn_idle_text };
                    if ui.add(egui::Button::new(egui::RichText::new(llama_lbl).strong().color(llama_txt)).fill(llama_fill).corner_radius(egui::CornerRadius::same(14))).clicked() {
                        if procs.llama {
                            let exes = collect_backend_exes(&self.cfg);
                            stop_all_backends(&exes);
                            self.loaded_label = None;
                            procs.llama = false;
                            self.set_procs(procs);
                            self.status_msg = "Stopped active inference engine — VRAM freed.".to_string();
                        } else if let Some(set) = self.cfg.routing_sets.get(self.cfg.selected_set_idx).cloned() {
                            self.sync_backend_exes();
                            match launch_routing_set(
                                &self.cfg,
                                &set,
                                Arc::clone(&self.llama_console),
                                Arc::clone(&self.error_logs),
                                ui.ctx().clone(),
                                llama_vp_id(),
                            ) {
                                Ok(()) => {
                                    self.loaded_label = Some(format!("Routing Set: {} [{}]", set.name, set.mode));
                                    procs.llama = true;
                                    self.set_procs(procs);
                                    self.status_msg = format!("Started Routing Set '{}' on :{}.", set.name, self.cfg.server_port);
                                }
                                Err(e) => self.push_error(e),
                            }
                        }
                    }

                    let fe_name = self.cfg.effective_frontend_name().to_string();
                    let fe_exe = self.cfg.effective_frontend_exe_name();
                    let owu_lbl = if procs.owu { format!("{fe_name} (:{}): ON", self.cfg.owu_port) } else { format!("{fe_name} (:{}): OFF", self.cfg.owu_port) };
                    let owu_fill = if procs.owu { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                    let owu_txt = if procs.owu { egui::Color32::WHITE } else { pal.btn_idle_text };
                    if ui.add(egui::Button::new(egui::RichText::new(owu_lbl).strong().color(owu_txt)).fill(owu_fill).corner_radius(egui::CornerRadius::same(14))).clicked() {
                        if procs.owu {
                            stop_process(&fe_exe);
                            procs.owu = false;
                            self.set_procs(procs);
                            self.status_msg = format!("Stopped {fe_name}.");
                        } else {
                            match launch_open_webui(&self.cfg, Arc::clone(&self.owu_console), Arc::clone(&self.error_logs), ui.ctx().clone(), owu_vp_id()) {
                                Ok(()) => { procs.owu = true; self.set_procs(procs); self.status_msg = format!("Started {fe_name} on port {}.", self.cfg.owu_port); }
                                Err(e) => self.push_error(e),
                            }
                        }
                    }

                    if cfg!(feature = "plus") {
                        let comp_snap = self.cfg.companion_services.clone();
                        for (ci, svc) in comp_snap.iter().enumerate() {
                            if !svc.enabled || svc.name.trim().is_empty() { continue; }
                            let is_on = ci < procs.companion_running.len() && procs.companion_running[ci];
                            let port_tag = if svc.check_port > 0 { format!(" (:{})", svc.check_port) } else { String::new() };
                            let lbl = if is_on { format!("{}{port_tag}: ON", svc.name) } else { format!("{}{port_tag}: OFF", svc.name) };
                            let fill = if is_on { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                            let txt = if is_on { egui::Color32::WHITE } else { pal.btn_idle_text };
                            if ui.add(egui::Button::new(egui::RichText::new(lbl).strong().color(txt)).fill(fill).corner_radius(egui::CornerRadius::same(14))).clicked() {
                                let res = if is_on { stop_companion_service(svc) } else { start_companion_service(svc) };
                                match res {
                                    Ok(()) => {
                                        if ci < procs.companion_running.len() { procs.companion_running[ci] = !is_on; }
                                        self.set_procs(procs);
                                        self.status_msg = format!("{} {}...", if is_on { "Stopping" } else { "Starting" }, svc.name);
                                    }
                                    Err(e) => self.push_error(e),
                                }
                            }
                        }
                        if self.cfg.enable_mcpo_toggle {
                            let mcpo_lbl = if procs.mcpo { format!("mcpo (:{}): ON", self.cfg.mcpo_port) } else { format!("mcpo (:{}): OFF", self.cfg.mcpo_port) };
                            let mcpo_fill = if procs.mcpo { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                            let mcpo_txt = if procs.mcpo { egui::Color32::WHITE } else { pal.btn_idle_text };
                            if ui.add(egui::Button::new(egui::RichText::new(mcpo_lbl).strong().color(mcpo_txt)).fill(mcpo_fill).corner_radius(egui::CornerRadius::same(14))).clicked() {
                                if procs.mcpo { stop_process(&platform::default_exe_name("mcpo")); procs.mcpo = false; self.set_procs(procs); }
                                else if let Err(e) = launch_mcpo(&self.cfg) { self.push_error(e); }
                                else { procs.mcpo = true; self.set_procs(procs); }
                            }
                        }
                    }

                    ui.separator();
                    let llama_win_open = self.show_llama_console.load(Ordering::Relaxed);
                    let owu_win_open = self.show_owu_console.load(Ordering::Relaxed);
                    let any_console_open = llama_win_open || owu_win_open;
                    let fe_con_tag = if fe_name == "Open WebUI" { "OWU" } else { "Frontend" };

                    let l_fill = if llama_win_open { pal.accent } else { pal.btn_idle };
                    let l_txt = if llama_win_open { pal.accent_text } else { pal.btn_idle_text };
                    if ui.add(egui::Button::new(egui::RichText::new(if llama_win_open { "Engine Console [ON]" } else { "Engine Console" }).strong().color(l_txt)).fill(l_fill)).clicked() {
                        self.show_llama_console.store(!llama_win_open, Ordering::Relaxed);
                    }
                    let o_fill = if owu_win_open { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                    let o_txt = if owu_win_open { egui::Color32::WHITE } else { pal.btn_idle_text };
                    if ui.add(egui::Button::new(egui::RichText::new(if owu_win_open { format!("{fe_con_tag} Console [ON]") } else { format!("{fe_con_tag} Console") }).strong().color(o_txt)).fill(o_fill)).clicked() {
                        self.show_owu_console.store(!owu_win_open, Ordering::Relaxed);
                    }
                    if ui.button(if any_console_open { "Close Both Consoles" } else { "Open Both Consoles" }).clicked() {
                        let next = !any_console_open;
                        self.show_llama_console.store(next, Ordering::Relaxed);
                        self.show_owu_console.store(next, Ordering::Relaxed);
                    }

                    ui.separator();
                    if ui.button("Open Browser").clicked() { platform::open_url(&self.cfg.effective_frontend_url()); }
                    if cfg!(feature = "plus") {
                        if !self.cfg.update_owu_bat.trim().is_empty() && ui.button("Update OWU").clicked() {
                            if let Err(e) = run_bat_file(&self.cfg.update_owu_bat) { self.push_error(e); } else { self.status_msg = "Launched Update OWU script.".to_string(); }
                        }
                        if !self.cfg.update_python_bat.trim().is_empty() && ui.button("Update Python").clicked() {
                            if let Err(e) = run_bat_file(&self.cfg.update_python_bat) { self.push_error(e); } else { self.status_msg = "Launched Update Python script.".to_string(); }
                        }
                        for sc in self.cfg.quick_scripts.clone() {
                            if sc.label.trim().is_empty() || sc.path.trim().is_empty() { continue; }
                            if ui.button(&sc.label).clicked() {
                                if let Err(e) = run_bat_file(&sc.path) { self.push_error(e); } else { self.status_msg = format!("Launched {}.", sc.label); }
                            }
                        }
                    }
                    let stop_all = egui::Button::new(egui::RichText::new("Stop All").strong().color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(145, 32, 38));
                    if ui.add(stop_all).clicked() {
                        let exes = collect_backend_exes(&self.cfg);
                        stop_all_backends(&exes);
                        stop_process(&fe_exe);
                        stop_process(&platform::default_exe_name("mcpo"));
                        if cfg!(feature = "plus") {
                            for (ci, svc) in self.cfg.companion_services.iter().enumerate() {
                                if svc.enabled && ci < procs.companion_running.len() && procs.companion_running[ci] {
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

            // =========================================================================
            // MAIN SPLIT: LEFT SELECTION WORKSPACE | RIGHT FULL-HEIGHT INSPECTOR
            // =========================================================================
                        let total_w = ui.available_width();
            let col_gap = 8.0;
            let left_workspace_w = ((total_w - col_gap) * 0.5).floor().max(340.0);
            let right_width = (total_w - col_gap - left_workspace_w).max(340.0);
            let col1_w = ((left_workspace_w - col_gap) * 0.48).floor().max(160.0);
            let col2_w = (left_workspace_w - col_gap - col1_w).max(160.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_workspace_w, avail_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_min_width(left_workspace_w);
                        ui.set_max_width(left_workspace_w);
                        let mut left_save_note: Option<String> = None;

                        // TOP CARD: ROUTING SETS BAR
                        themed_card(pal).show(ui, |ui| {
                            let inner_w = (left_workspace_w - 26.0).max(200.0);
                            ui.set_min_width(inner_w);
                            ui.set_max_width(inner_w);
                            let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };

                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(
                                    egui::RichText::new("ROUTING SETS (OWU MULTI-ENGINE SWITCHING)")
                                        .size(11.5)
                                        .strong()
                                        .color(hdr_col),
                                ).selectable(false));

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if let Some(s) = self.cfg.routing_sets.get(self.cfg.selected_set_idx) {
                                        let mtag = if s.mode == "llama_ini" { "models.ini" } else { "llama-swap" };
                                        ui.label(egui::RichText::new(format!("Router: {mtag}")).size(10.5).color(pal.accent));
                                    }
                                });
                            });

                            ui.add_space(4.0);

                            // Set Selector Pills + Edit Mode Add/Del
                            let mut select_set_idx: Option<usize> = None;
                            let mut del_active_set = false;
                            ui.horizontal_wrapped(|ui| {
                                for (sidx, set) in self.cfg.routing_sets.iter().enumerate() {
                                    let is_active = sidx == self.cfg.selected_set_idx;
                                    let pill = egui::Button::new(
                                        egui::RichText::new(format!("{} ({})", set.name, set.profile_ids.len()))
                                            .size(12.0)
                                            .strong()
                                            .color(if is_active { pal.accent_text } else { pal.btn_idle_text }),
                                    )
                                    .fill(if is_active { pal.accent } else { pal.btn_idle })
                                    .corner_radius(egui::CornerRadius::same(8));
                                    if ui.add(pill).on_hover_text(&set.description).clicked() {
                                        select_set_idx = Some(sidx);
                                    }
                                }

                                if edit_mode {
                                    pasteable_singleline(ui, &mut self.new_set_name, 130.0, "New Set...");
                                    if ui.button("+ Set").clicked() && !self.new_set_name.trim().is_empty() {
                                        let name = self.new_set_name.trim().to_string();
                                        let id = format!("{}-{}", slugify(&name), self.cfg.routing_sets.len() + 1);
                                        self.cfg.routing_sets.push(RoutingSet {
                                            id,
                                            name: name.clone(),
                                            description: "Cross-family multi-engine routing set.".to_string(),
                                            mode: "llama_swap".to_string(),
                                            no_models_autoload: false,
                                            profile_ids: Vec::new(),
                                        });
                                        self.cfg.selected_set_idx = self.cfg.routing_sets.len() - 1;
                                        self.new_set_name.clear();
                                        left_save_note = Some(format!("Created Routing Set '{name}'."));
                                    }
                                    if self.cfg.routing_sets.len() > 1 {
                                        let del_btn = egui::Button::new(egui::RichText::new("Del").color(egui::Color32::WHITE))
                                            .fill(egui::Color32::from_rgb(145, 32, 38));
                                        if ui.add(del_btn).on_hover_text("Delete active Routing Set").clicked() {
                                            del_active_set = true;
                                        }
                                    }
                                }
                            });

                            if let Some(sidx) = select_set_idx {
                                self.cfg.selected_set_idx = sidx;
                                let _ = self.cfg.save();
                                self.saved_cfg = self.cfg.clone();
                            }
                            if del_active_set && self.cfg.routing_sets.len() > 1 {
                                let removed = self.cfg.routing_sets.remove(self.cfg.selected_set_idx);
                                if self.cfg.selected_set_idx >= self.cfg.routing_sets.len() {
                                    self.cfg.selected_set_idx = self.cfg.routing_sets.len() - 1;
                                }
                                left_save_note = Some(format!("Deleted Routing Set '{}'.", removed.name));
                            }

                            // Edit Active Set Details (Edit Mode only)
                            if edit_mode {
                                if let Some(s_mut) = self.cfg.routing_sets.get_mut(self.cfg.selected_set_idx) {
                                    ui.add_space(4.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Name:");
                                        pasteable_singleline(ui, &mut s_mut.name, 145.0, "");
                                        ui.label("Router:");
                                        egui::ComboBox::from_id_salt("active_set_mode")
                                            .selected_text(if s_mut.mode == "llama_ini" { "models.ini" } else { "llama-swap" })
                                            .width(96.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut s_mut.mode, "llama_swap".to_string(), "llama-swap (Multi-Engine)");
                                                ui.selectable_value(&mut s_mut.mode, "llama_ini".to_string(), "llama.cpp models.ini");
                                            });
                                        if s_mut.mode == "llama_ini" {
                                            ui.checkbox(&mut s_mut.no_models_autoload, "No Autoload");
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Desc:");
                                        pasteable_singleline(ui, &mut s_mut.description, (left_workspace_w - 94.0).max(120.0), "");
                                    });
                                }
                            }

                            ui.add_space(5.0);

                            // Linked Model Chips (click chip -> jumps straight to Family & Sub-Profile)
                            let mut unlink_req: Option<usize> = None;
                            let mut move_link_req: Option<(usize, usize)> = None;
                            let mut jump_to_pid: Option<String> = None;

                            let cur_selected_pid = self
                                .cfg
                                .current_profile()
                                .map(|p| p.id.clone())
                                .unwrap_or_default();

                            if let Some(active_set) = self.cfg.routing_sets.get(self.cfg.selected_set_idx).cloned() {
                                if !edit_mode && !active_set.description.is_empty() {
                                    ui.add(egui::Label::new(
                                        egui::RichText::new(&active_set.description).size(11.0).color(pal.muted),
                                    ).selectable(false));
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
                                                    let (short_label, hover_tip, eng_lbl) = match self.cfg.find_profile_with_group(pid) {
                                                        Some((g, p)) => (
                                                            p.swap_model_id(),
                                                            format!("{} -> {} [{}]\nClick to select & inspect", g.model_title, p.name, p.engine_badge_name()),
                                                            p.engine_badge_name().to_string(),
                                                        ),
                                                        None => (
                                                            pid.clone(),
                                                            "Missing profile reference".to_string(),
                                                            "missing".to_string(),
                                                        ),
                                                    };
                                                    let ecol = engine_badge_color_for(&eng_lbl, &self.cfg.saved_engines);
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

                                                    let chip_fill = if is_cur { pal.item_sel_bg } else { pal.code_bg };
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
                                                                if li + 1 < lcount && ui.small_button(">").clicked() {
                                                                    move_link_req = Some((li, li + 1));
                                                                }
                                                                if ui.small_button("x").on_hover_text("Unlink").clicked() {
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
                                if let Some((gi, pi)) = self.cfg.find_profile_pos(&pid) {
                                    self.cfg.selected_group_idx = gi;
                                    self.cfg.selected_profile_idx = pi;
                                    if let Some(p) = self.cfg.groups.get(gi).and_then(|g| g.profiles.get(pi)) {
                                        self.status_msg = format!("Selected '{}' from Routing Set.", p.name);
                                    }
                                }
                            }
                            if let Some(li) = unlink_req {
                                if let Some(set) = self.cfg.routing_sets.get_mut(self.cfg.selected_set_idx) {
                                    set.profile_ids.remove(li);
                                    left_save_note = Some("Unlinked profile from Routing Set.".to_string());
                                }
                            }
                            if let Some((a, b)) = move_link_req {
                                if let Some(set) = self.cfg.routing_sets.get_mut(self.cfg.selected_set_idx) {
                                    set.profile_ids.swap(a, b);
                                    left_save_note = Some("Reordered linked profiles in Routing Set.".to_string());
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
                                                    let needs_setup = self.cfg.models_dir.trim().is_empty() && self.cfg.llama_dir.trim().is_empty();
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
                                                        ui.label("llama-swap.exe (Router):");
                                                        ui.horizontal(|ui| {
                                                            pasteable_singleline(ui, &mut self.cfg.llama_swap_exe, 185.0, "");
                                                            if ui.button("...").clicked() {
                                                                if let Some(p) = FileDialog::new().add_filter("Executable", &["exe"]).pick_file() {
                                                                    self.cfg.llama_swap_exe = p.to_string_lossy().to_string();
                                                                    left_save_note = Some("Updated llama-swap.exe path.".to_string());
                                                                }
                                                            }
                                                        });

                                                        ui.label("Global Engines Folder:");
                                                        ui.horizontal(|ui| {
                                                            pasteable_singleline(ui, &mut self.cfg.llama_dir, 185.0, "");
                                                            if ui.button("...").clicked() {
                                                                if let Some(p) = FileDialog::new().pick_folder() {
                                                                    self.cfg.llama_dir = p.to_string_lossy().to_string();
                                                                    left_save_note = Some("Updated Global Engines folder.".to_string());
                                                                }
                                                            }
                                                        });

                                                        if cfg!(feature = "plus") {
    ui.label("External help.md:");
    ui.horizontal(|ui| {
        pasteable_singleline(ui, &mut self.cfg.help_md_path, 185.0, "");
        if ui.button("...").clicked() {
            if let Some(p) = FileDialog::new().add_filter("Markdown", &["md", "txt"]).pick_file() {
                self.cfg.help_md_path = p.to_string_lossy().to_string();
                self.help_map = Self::load_help_with_fallback(&self.cfg.help_md_path, self.patch.embedded_help_md);
                left_save_note = Some("Updated external help.md path.".to_string());
            }
        }
    });
    ui.separator();
    egui::CollapsingHeader::new(egui::RichText::new("Companion Services").size(10.5).strong().color(pal.edit_hl)).default_open(false).show(ui, |ui| {
        let mut del_svc: Option<usize> = None;
        let mut svc_dirty = false;
        for (si, svc) in self.cfg.companion_services.iter_mut().enumerate() {
            egui::Frame::new().fill(pal.item_bg).stroke(egui::Stroke::new(1.0, pal.item_border)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut svc.enabled, "").on_hover_text("Show toggle pill in header").changed() { svc_dirty = true; }
                    if pasteable_singleline(ui, &mut svc.name, 105.0, "Name (e.g. SearXNG)").lost_focus() { svc_dirty = true; }
                    ui.label("Port:");
                    if ui.add(egui::DragValue::new(&mut svc.check_port).speed(1.0)).changed() { svc_dirty = true; }
                    if ui.small_button("X").clicked() { del_svc = Some(si); }
                });
                ui.horizontal(|ui| {
                    ui.label("Start:");
                    if pasteable_singleline(ui, &mut svc.start_cmd, 150.0, "start script...").lost_focus() { svc_dirty = true; }
                    if ui.small_button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_file() { svc.start_cmd = p.to_string_lossy().to_string(); svc_dirty = true; }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Stop: ");
                    if pasteable_singleline(ui, &mut svc.stop_cmd, 150.0, "stop script...").lost_focus() { svc_dirty = true; }
                    if ui.small_button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_file() { svc.stop_cmd = p.to_string_lossy().to_string(); svc_dirty = true; }
                    }
                });
            });
            ui.add_space(3.0);
        }
        if let Some(i) = del_svc { self.cfg.companion_services.remove(i); svc_dirty = true; }
        ui.horizontal(|ui| {
            if ui.small_button("+ Add Service").clicked() {
                self.cfg.companion_services.push(config::CompanionService { name: "New Service".to_string(), start_cmd: String::new(), stop_cmd: String::new(), check_port: 8888, url: String::new(), enabled: true });
                svc_dirty = true;
            }
            if ui.small_button("Save").clicked() { svc_dirty = true; }
        });
        if svc_dirty { self.sync_backend_exes(); left_save_note = Some("Updated Companion Services.".to_string()); }
    });
    egui::CollapsingHeader::new(egui::RichText::new("Quick-Action Scripts").size(10.5).strong().color(pal.edit_hl)).default_open(false).show(ui, |ui| {
        let mut del_sc: Option<usize> = None;
        let mut sc_dirty = false;
        for (qi, sc) in self.cfg.quick_scripts.iter_mut().enumerate() {
            egui::Frame::new().fill(pal.item_bg).stroke(egui::Stroke::new(1.0, pal.item_border)).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if pasteable_singleline(ui, &mut sc.label, 170.0, "Button label...").lost_focus() { sc_dirty = true; }
                    if ui.small_button("X").clicked() { del_sc = Some(qi); }
                });
                ui.horizontal(|ui| {
                    if pasteable_singleline(ui, &mut sc.path, 165.0, "script path...").lost_focus() { sc_dirty = true; }
                    if ui.small_button("...").clicked() {
                        if let Some(p) = FileDialog::new().pick_file() { sc.path = p.to_string_lossy().to_string(); sc_dirty = true; }
                    }
                });
            });
            ui.add_space(3.0);
        }
        if let Some(i) = del_sc { self.cfg.quick_scripts.remove(i); sc_dirty = true; }
        ui.horizontal(|ui| {
            if ui.small_button("+ Add Script").clicked() {
                self.cfg.quick_scripts.push(config::QuickScript { label: "Run Script".to_string(), path: String::new() });
                sc_dirty = true;
            }
            if ui.small_button("Save").clicked() { sc_dirty = true; }
        });
        if sc_dirty { left_save_note = Some("Updated Quick-Action Scripts.".to_string()); }
    });
}

                                                        ui.separator();
                                                        ui.label(egui::RichText::new("SAVED ENGINES LIBRARY").size(10.5).strong().color(pal.edit_hl));
                                                        let fallback_dir = self.cfg.llama_dir.clone();
                                                        let mut del_eng_idx: Option<usize> = None;
                                                        for (eidx, eng) in self.cfg.saved_engines.iter_mut().enumerate() {
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
                                                                            left_save_note = Some(format!("Updated color for '{}'.", eng.label));
                                                                        }
                                                                        pasteable_singleline(ui, &mut eng.label, 145.0, "Engine name...");
                                                                        if ui.small_button("X").clicked() { del_eng_idx = Some(eidx); }
                                                                    });
                                                                    ui.horizontal(|ui| {
                                                                        let cm = if eng.engine_kind == "custom" { "custom" } else { "llama_cpp" };
                                                                        egui::ComboBox::from_id_salt(("eng_mode", eidx)).selected_text(cm).width(78.0).show_ui(ui, |ui| {
                                                                            ui.selectable_value(&mut eng.engine_kind, "llama_cpp".to_string(), "llama_cpp");
                                                                            ui.selectable_value(&mut eng.engine_kind, "custom".to_string(), "custom");
                                                                        });
                                                                        pasteable_singleline(ui, &mut eng.exe_name, 88.0, "exe...");
                                                                        if ui.small_button(".exe").clicked() {
                                                                            let start = if eng.dir_path.trim().is_empty() { fallback_dir.as_str() } else { eng.dir_path.as_str() };
                                                                            let mut dlg = FileDialog::new(); if cfg!(windows) { dlg = dlg.add_filter("Executable", &["exe"]); }
                                                                            if Path::new(start).exists() { dlg = dlg.set_directory(start); }
                                                                            if let Some(p) = dlg.pick_file() {
                                                                                if let Some(par) = p.parent() { eng.dir_path = par.to_string_lossy().to_string(); }
                                                                                if let Some(fname) = p.file_name() { eng.exe_name = fname.to_string_lossy().to_string(); }
                                                                                left_save_note = Some(format!("Updated engine '{}'.", eng.label));
                                                                            }
                                                                        }
                                                                    });
                                                                    ui.horizontal(|ui| {
                                                                        pasteable_singleline(ui, &mut eng.dir_path, 178.0, "Folder...");
                                                                        if ui.small_button("...").clicked() {
                                                                            let start = if eng.dir_path.trim().is_empty() { fallback_dir.as_str() } else { eng.dir_path.as_str() };
                                                                            let mut dlg = FileDialog::new();
                                                                            if Path::new(start).exists() { dlg = dlg.set_directory(start); }
                                                                            if let Some(p) = dlg.pick_folder() {
                                                                                eng.dir_path = p.to_string_lossy().to_string();
                                                                                left_save_note = Some(format!("Updated folder for '{}'.", eng.label));
                                                                            }
                                                                        }
                                                                    });
                                                                    if eng.engine_kind == "custom" {
                                                                        ui.horizontal(|ui| {
                                                                            ui.label(egui::RichText::new("Args:").size(10.0).color(pal.muted));
                                                                            if pasteable_singleline(ui, &mut eng.default_args, 165.0, "serve --model {model} --ctx {ctx} --addr {addr}").lost_focus() {
                                                                                left_save_note = Some(format!("Updated default args for '{}'.", eng.label));
                                                                            }
                                                                        });
                                                                    }
                                                                });
                                                            ui.add_space(3.0);
                                                        }
                                                        if let Some(di) = del_eng_idx {
                                                            let removed = self.cfg.saved_engines.remove(di);
                                                            left_save_note = Some(format!("Removed saved engine '{}'.", removed.label));
                                                        }

                                                        ui.add_space(4.0);
                                                        ui.label(egui::RichText::new("Add New Engine:").size(10.5).strong());
                                                        ui.horizontal(|ui| {
                                                            ui.color_edit_button_srgb(&mut self.new_engine_color).on_hover_text("Pick color for this engine");
                                                            pasteable_singleline(ui, &mut self.new_engine_label, 182.0, "Engine name...");
                                                        });
                                                        ui.horizontal(|ui| {
                                                            let cm = if self.new_engine_kind == "custom" { "custom" } else { "llama_cpp" };
                                                            egui::ComboBox::from_id_salt("new_eng_kind").selected_text(cm).width(78.0).show_ui(ui, |ui| {
                                                                ui.selectable_value(&mut self.new_engine_kind, "llama_cpp".to_string(), "llama_cpp");
                                                                if ui.selectable_value(&mut self.new_engine_kind, "custom".to_string(), "custom").clicked()
                                                                    && self.new_engine_args.trim().is_empty()
                                                                {
                                                                    self.new_engine_args = "serve --model {model} --ctx {ctx} --addr {addr}".to_string();
                                                                }
                                                            });
                                                            pasteable_singleline(ui, &mut self.new_engine_exe, 88.0, "exe...");
                                                            if ui.small_button(".exe").clicked() {
                                                                let mut dlg = FileDialog::new();
                                                                if cfg!(windows) { dlg = dlg.add_filter("Executable", &["exe"]); }
                                                                if let Some(p) = dlg.pick_file() {
                                                                    if let Some(par) = p.parent() { self.new_engine_dir = par.to_string_lossy().to_string(); }
                                                                    if let Some(fname) = p.file_name() { self.new_engine_exe = fname.to_string_lossy().to_string(); }
                                                                    if self.new_engine_label.trim().is_empty() {
                                                                        if let Some(stem) = p.file_stem() { self.new_engine_label = stem.to_string_lossy().to_string(); }
                                                                    }
                                                                }
                                                            }
                                                        });
                                                        if self.new_engine_kind == "custom" {
                                                            ui.horizontal(|ui| {
                                                                ui.label(egui::RichText::new("Args:").size(10.0).color(pal.muted));
                                                                pasteable_singleline(ui, &mut self.new_engine_args, 175.0, "serve --model {model} --ctx {ctx} --addr {addr}");
                                                            });
                                                        }
                                                        ui.horizontal(|ui| {
                                                            pasteable_singleline(ui, &mut self.new_engine_dir, 140.0, "Folder...");
                                                            if ui.small_button("...").clicked() {
                                                                if let Some(p) = FileDialog::new().pick_folder() {
                                                                    self.new_engine_dir = p.to_string_lossy().to_string();
                                                                }
                                                            }
                                                            if ui.small_button("+ Save").clicked()
                                                                && !self.new_engine_label.trim().is_empty()
                                                                && !self.new_engine_dir.trim().is_empty()
                                                            {
                                                                let lbl = self.new_engine_label.trim().to_string();
                                                                let exe_n = if self.new_engine_exe.trim().is_empty() {
                                                                    platform::default_exe_name("llama-server")
                                                                } else {
                                                                    self.new_engine_exe.trim().to_string()
                                                                };
                                                                let def_args = if self.new_engine_kind == "custom" {
                                                                    self.new_engine_args.trim().to_string()
                                                                } else {
                                                                    String::new()
                                                                };
                                                                self.cfg.saved_engines.push(EnginePreset {
                                                                    label: lbl.clone(),
                                                                    engine_kind: self.new_engine_kind.clone(),
                                                                    dir_path: self.new_engine_dir.trim().to_string(),
                                                                    exe_name: exe_n,
                                                                    color_rgb: Some(self.new_engine_color),
                                                                    default_args: def_args,
                                                                });
                                                                let next_idx = self.cfg.saved_engines.len() % ENGINE_COLOR_PALETTE.len();
                                                                self.new_engine_color = ENGINE_COLOR_PALETTE[next_idx];
                                                                self.new_engine_label.clear();
                                                                self.new_engine_dir.clear();
                                                                self.new_engine_args.clear();
                                                                self.new_engine_exe = platform::default_exe_name("llama-server");
                                                                left_save_note = Some(format!("Saved engine '{lbl}'."));
                                                            }
                                                        });

                                                        ui.separator();
                                                        ui.label("Models Folder (.gguf):");
                                                        ui.horizontal(|ui| {
                                                            pasteable_singleline(ui, &mut self.cfg.models_dir, 185.0, "");
                                                            if ui.button("...").clicked() {
                                                                if let Some(p) = FileDialog::new().pick_folder() {
                                                                    self.cfg.models_dir = p.to_string_lossy().to_string();
                                                                    self.scanned_models = scan_gguf_models(&self.cfg.models_dir);
                                                                    self.gguf_cache.clear();
                                                                    left_save_note = Some("Updated Models folder.".to_string());
                                                                }
                                                            }
                                                        });
                                                        ui.separator();
ui.label(egui::RichText::new("FRONTEND (CHAT UI)").size(10.5).strong().color(pal.edit_hl));
let mut fe_dirty = false;
ui.horizontal(|ui| {
    ui.label("Name:");
    if pasteable_singleline(ui, &mut self.cfg.frontend_name, 145.0, "e.g. Open WebUI, SillyTavern").lost_focus() { fe_dirty = true; }
});
ui.label("Frontend Executable:");
ui.horizontal(|ui| {
    if pasteable_singleline(ui, &mut self.cfg.frontend_exe, 185.0, "open-webui.exe / script...").lost_focus() { fe_dirty = true; }
    if ui.button("...").clicked() {
        if let Some(p) = FileDialog::new().pick_file() {
            self.cfg.frontend_exe = p.to_string_lossy().to_string();
            fe_dirty = true;
        }
    }
});
ui.label("Frontend Folder (optional):");
ui.horizontal(|ui| {
    if pasteable_singleline(ui, &mut self.cfg.owu_scripts_dir, 185.0, "Fallback dir (e.g. Scripts/)").lost_focus() { fe_dirty = true; }
    if ui.button("...").clicked() {
        if let Some(p) = FileDialog::new().pick_folder() {
            self.cfg.owu_scripts_dir = p.to_string_lossy().to_string();
            fe_dirty = true;
        }
    }
});
ui.horizontal(|ui| {
    ui.label("Args:");
    if pasteable_singleline(ui, &mut self.cfg.frontend_args, 175.0, "serve --port {port}").lost_focus() { fe_dirty = true; }
});
ui.horizontal(|ui| {
    ui.label("URL: ");
    if pasteable_singleline(ui, &mut self.cfg.frontend_url, 175.0, "http://127.0.0.1:{port}").lost_focus() { fe_dirty = true; }
});
if fe_dirty {
    self.sync_backend_exes();
    left_save_note = Some("Updated Frontend settings.".to_string());
}
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label("API:");
                                                            ui.add(egui::DragValue::new(&mut self.cfg.server_port).range(1024..=65535));
                                                            ui.label("Frontend:");
                                                            ui.add(egui::DragValue::new(&mut self.cfg.owu_port).range(1024..=65535));
                                                            if cfg!(feature = "plus") { ui.checkbox(&mut self.cfg.enable_mcpo_toggle, "mcpo"); }
                                                        });
                                                        if ui.checkbox(&mut self.cfg.use_external_cmd_windows, "Use External CMD").changed() {
                                                            left_save_note = Some("Saved console mode preference.".to_string());
                                                        }
                                                            });
    });
ui.add_space(4.0);
                                                    ui.add_space(6.0);
                                                }

                                                let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };
                                                ui.add(egui::Label::new(
                                                    egui::RichText::new("1. MODEL FAMILIES").size(12.0).strong().color(hdr_col),
                                                ).selectable(false));

                                                if edit_mode {
                                                    ui.horizontal(|ui| {
                                                        pasteable_singleline(ui, &mut self.new_group_name, (col1_w - 108.0).max(80.0), "New Family...");
                                                        if ui.button("+ Add").clicked() && !self.new_group_name.trim().is_empty() {
                                                            let title = self.new_group_name.trim().to_string();
                                                            let uid = self.cfg.make_unique_profile_id(&title);
                                                            let mut default_prof = ModelProfile::default();
                                                            default_prof.id = uid;
                                                            default_prof.help_key = guess_help_key(&title, "");
                                                            self.cfg.groups.push(ModelGroup {
                                                                model_title: title.clone(),
                                                                subtitle: "Custom GGUF Family".to_string(),
                                                                profiles: vec![default_prof],
                                                            });
                                                            self.cfg.selected_group_idx = self.cfg.groups.len() - 1;
                                                            self.cfg.selected_profile_idx = 0;
                                                            self.new_group_name.clear();
                                                            left_save_note = Some(format!("Added Model Family '{title}'."));
                                                        }
                                                    });
                                                }

                                                ui.add_space(4.0);
                                                let mut swap_groups: Option<(usize, usize)> = None;
                                                let mut del_group_idx: Option<usize> = None;
                                                let glen = self.cfg.groups.len();

                                                for idx in 0..glen {
                                                    let is_sel = self.cfg.selected_group_idx == idx;
                                                    let title = self.cfg.groups[idx].model_title.clone();
                                                    let sub = self.cfg.groups[idx].subtitle.clone();
                                                    let count = self.cfg.groups[idx].profiles.len();

                                                    let (cfill, cborder) = if is_sel {
                                                        (pal.item_sel_bg, pal.item_sel_border)
                                                    } else {
                                                        (pal.item_bg, pal.item_border)
                                                    };

                                                    egui::Frame::new()
                                                        .fill(cfill)
                                                        .stroke(egui::Stroke::new(if is_sel { 1.5 } else { 1.0 }, cborder))
                                                        .corner_radius(egui::CornerRadius::same(10))
                                                        .inner_margin(egui::Margin::same(8))
                                                        .show(ui, |ui| {
                                                            ui.set_min_width((col1_w - 44.0).max(140.0));
                                                            // Full-width clickable family header + subtitle (zero overlap)
                                                            let inner = ui.vertical(|ui| {
                                                                ui.add(egui::Label::new(
                                                                    egui::RichText::new(&title).strong().color(pal.text),
                                                                ).selectable(false));
                                                                ui.add(egui::Label::new(
                                                                    egui::RichText::new(format!("{sub} ({count})"))
                                                                        .size(11.0)
                                                                        .color(pal.muted),
                                                                ).selectable(false));
                                                            });
                                                            if ui.interact(inner.response.rect, ui.scope_id().with(("cc_grp", idx)), egui::Sense::click()).clicked() {
                                                                if self.cfg.selected_group_idx != idx {
                                                                    self.cfg.selected_group_idx = idx;
                                                                    self.cfg.selected_profile_idx = 0;
                                                                    let _ = self.cfg.save();
                                                                    self.saved_cfg = self.cfg.clone();
                                                                }
                                                            }

                                                            // Dedicated bottom strip in Edit Mode (never covers subtitle)
                                                            if edit_mode {
                                                                ui.add_space(4.0);
                                                                ui.separator();
                                                                ui.horizontal(|ui| {
                                                                    if ui.add_enabled(idx > 0, egui::Button::new("^").min_size(egui::vec2(24.0, 18.0))).clicked() {
                                                                        swap_groups = Some((idx, idx - 1));
                                                                    }
                                                                    if ui.add_enabled(idx + 1 < glen, egui::Button::new("v").min_size(egui::vec2(24.0, 18.0))).clicked() {
                                                                        swap_groups = Some((idx, idx + 1));
                                                                    }
                                                                    if glen > 1 {
                                                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                            let del_b = egui::Button::new(egui::RichText::new("Del").size(11.0).color(egui::Color32::WHITE))
                                                                                .fill(egui::Color32::from_rgb(145, 32, 38));
                                                                            if ui.add(del_b).clicked() {
                                                                                del_group_idx = Some(idx);
                                                                            }
                                                                        });
                                                                    }
                                                                });
                                                            }
                                                        });
                                                    ui.add_space(4.0);
                                                }

                                                if let Some((a, b)) = swap_groups {
                                                    self.cfg.groups.swap(a, b);
                                                    if self.cfg.selected_group_idx == a { self.cfg.selected_group_idx = b; }
                                                    else if self.cfg.selected_group_idx == b { self.cfg.selected_group_idx = a; }
                                                    left_save_note = Some("Reordered Model Families and saved.".to_string());
                                                }
                                                if let Some(di) = del_group_idx {
                                                    if self.cfg.groups.len() > 1 {
                                                        let removed = self.cfg.groups.remove(di);
                                                        let removed_ids: Vec<String> = removed.profiles.iter().map(|p| p.id.clone()).collect();
                                                        for s in &mut self.cfg.routing_sets {
                                                            s.profile_ids.retain(|id| !removed_ids.contains(id));
                                                        }
                                                        if self.cfg.selected_group_idx >= self.cfg.groups.len() {
                                                            self.cfg.selected_group_idx = self.cfg.groups.len() - 1;
                                                        }
                                                        self.cfg.selected_profile_idx = 0;
                                                        left_save_note = Some(format!("Deleted Model Family '{}'.", removed.model_title));
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
                                        let g_idx = self.cfg.selected_group_idx;
                                        let g_title = self
                                            .cfg
                                            .groups
                                            .get(g_idx)
                                            .map(|g| g.model_title.clone())
                                            .unwrap_or_default();
                                        let hdr_col = if edit_mode { pal.edit_hl } else { pal.muted };
                                        let sub_col = if edit_mode { pal.edit_hl } else { pal.accent };

                                        ui.add(egui::Label::new(
                                            egui::RichText::new("2. SUB-PROFILES").size(12.0).strong().color(hdr_col),
                                        ).selectable(false));
                                        ui.add(egui::Label::new(
                                            egui::RichText::new(&g_title).size(12.5).strong().color(sub_col),
                                        ).selectable(false));
                                        ui.add_space(4.0);

                                        if edit_mode {
                                            ui.horizontal(|ui| {
                                                pasteable_singleline(ui, &mut self.new_profile_name, (col2_w - 108.0).max(80.0), "New sub-profile...");
                                                if ui.button("+ Add").clicked() && !self.new_profile_name.trim().is_empty() {
                                                    let pname = self.new_profile_name.trim().to_string();
                                                    let new_id = self.cfg.make_unique_profile_id(&format!("{g_title}-{pname}"));
                                                    if let Some(grp) = self.cfg.groups.get_mut(g_idx) {
                                                        let mut base = grp
                                                            .profiles
                                                            .get(self.cfg.selected_profile_idx)
                                                            .cloned()
                                                            .unwrap_or_default();
                                                        base.id = new_id;
                                                        base.name = pname.clone();
                                                        grp.profiles.push(base);
                                                        self.cfg.selected_profile_idx = grp.profiles.len() - 1;
                                                        self.new_profile_name.clear();
                                                        left_save_note = Some(format!("Added sub-profile '{pname}'."));
                                                    }
                                                }
                                            });
                                            ui.add_space(4.0);
                                        }

                                        let active_links: Vec<String> = self
                                            .cfg
                                            .routing_sets
                                            .get(self.cfg.selected_set_idx)
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
                                                if let Some(grp) = self.cfg.groups.get(g_idx) {
                                                    let plen = grp.profiles.len();
                                                    for pidx in 0..plen {
                                                        let p = &grp.profiles[pidx];
                                                        let is_sel = self.cfg.selected_profile_idx == pidx;
                                                        let pid = p.id.clone();
                                                        let pname = p.name.clone();
                                                        let prole = p.role_tag.clone();
                                                        let eng_badge = p.engine_badge_name().to_string();
                                                        let eng_col = engine_badge_color_for(&eng_badge, &self.cfg.saved_engines);
                                                        let is_linked = active_links.contains(&pid);

                                                        let (cfill, cborder) = if is_sel {
                                                            (pal.sub_sel_bg, pal.sub_sel_border)
                                                        } else {
                                                            (pal.item_bg, pal.item_border)
                                                        };

                                                        egui::Frame::new()
                                                            .fill(cfill)
                                                            .stroke(egui::Stroke::new(if is_sel { 1.5 } else { 1.0 }, cborder))
                                                            .corner_radius(egui::CornerRadius::same(10))
                                                            .inner_margin(egui::Margin::same(8))
                                                            .show(ui, |ui| {
                                                                ui.set_min_width((col2_w - 44.0).max(150.0));

                                                                // TIER 1 & 2: Full-width clickable title, badges, and description (NEVER overlapped)
                                                                let inner = ui.vertical(|ui| {
                                                                    ui.add(egui::Label::new(
                                                                        egui::RichText::new(&pname).strong().color(pal.text),
                                                                    ).selectable(false));
                                                                    ui.horizontal_wrapped(|ui| {
                                                                        ui.add(egui::Label::new(
                                                                            egui::RichText::new(format!("[{eng_badge}]"))
                                                                                .size(10.5)
                                                                                .strong()
                                                                                .color(eng_col),
                                                                        ).selectable(false));
                                                                        if is_linked {
                                                                            ui.add(egui::Label::new(
                                                                                egui::RichText::new("• In Active Set")
                                                                                    .size(10.5)
                                                                                    .color(egui::Color32::from_rgb(52, 211, 153)),
                                                                            ).selectable(false));
                                                                        }
                                                                    });
                                                                    if !prole.is_empty() {
                                                                        ui.add(egui::Label::new(
                                                                            egui::RichText::new(&prole).size(11.0).color(pal.muted),
                                                                        ).selectable(false));
                                                                    }
                                                                });
                                                                if ui.interact(inner.response.rect, ui.scope_id().with(("cc_subprof", g_idx, pidx)), egui::Sense::click()).clicked() {
                                                                    self.cfg.selected_profile_idx = pidx;
                                                                }

                                                                // TIER 3: Dedicated Edit Mode bottom strip (zero overlap with description)
                                                                if edit_mode {
                                                                    ui.add_space(4.0);
                                                                    ui.separator();
                                                                    ui.horizontal_wrapped(|ui| {
                                                                        let link_lbl = if is_linked { "Unlink" } else { "+ Link" };
                                                                        let link_fill = if is_linked { egui::Color32::from_rgb(16, 125, 78) } else { pal.btn_idle };
                                                                        let link_txt = if is_linked { egui::Color32::WHITE } else { pal.btn_idle_text };
                                                                        if ui.add(egui::Button::new(egui::RichText::new(link_lbl).size(11.0).color(link_txt)).fill(link_fill)).clicked() {
                                                                            toggle_link_pid = Some(pid.clone());
                                                                        }
                                                                        if ui.add_enabled(pidx > 0, egui::Button::new("^").min_size(egui::vec2(22.0, 18.0))).clicked() {
                                                                            swap_profs = Some((pidx, pidx - 1));
                                                                        }
                                                                        if ui.add_enabled(pidx + 1 < plen, egui::Button::new("v").min_size(egui::vec2(22.0, 18.0))).clicked() {
                                                                            swap_profs = Some((pidx, pidx + 1));
                                                                        }
                                                                        if ui.small_button("Dup").clicked() {
                                                                            dup_prof = Some(pidx);
                                                                        }
                                                                        if plen > 1 {
                                                                            let del_b = egui::Button::new(egui::RichText::new("Del").size(11.0).color(egui::Color32::WHITE))
                                                                                .fill(egui::Color32::from_rgb(145, 32, 38));
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
                                            if let Some(set) = self.cfg.routing_sets.get_mut(self.cfg.selected_set_idx) {
                                                if let Some(pos) = set.profile_ids.iter().position(|x| x == &pid) {
                                                    set.profile_ids.remove(pos);
                                                    left_save_note = Some("Unlinked sub-profile from active Routing Set.".to_string());
                                                } else {
                                                    set.profile_ids.push(pid);
                                                    left_save_note = Some("Linked sub-profile to active Routing Set.".to_string());
                                                }
                                            }
                                        }
                                        if let Some((a, b)) = swap_profs {
                                            if let Some(grp) = self.cfg.groups.get_mut(g_idx) {
                                                grp.profiles.swap(a, b);
                                                if self.cfg.selected_profile_idx == a { self.cfg.selected_profile_idx = b; }
                                                else if self.cfg.selected_profile_idx == b { self.cfg.selected_profile_idx = a; }
                                                left_save_note = Some("Reordered sub-profiles and saved.".to_string());
                                            }
                                        }
                                        if let Some(di) = dup_prof {
                                            if let Some(orig) = self.cfg.groups.get(g_idx).and_then(|g| g.profiles.get(di)).cloned() {
                                                let new_id = self.cfg.make_unique_profile_id(&format!("{}-copy", orig.id));
                                                let mut copy = orig;
                                                copy.id = new_id;
                                                copy.name = format!("{} (Copy)", copy.name);
                                                if let Some(grp) = self.cfg.groups.get_mut(g_idx) {
                                                    grp.profiles.insert(di + 1, copy);
                                                    self.cfg.selected_profile_idx = di + 1;
                                                    left_save_note = Some("Duplicated sub-profile.".to_string());
                                                }
                                            }
                                        }
                                        if let Some(di) = del_prof {
                                            let mut removed_info: Option<(String, String)> = None;
                                            if let Some(grp) = self.cfg.groups.get_mut(g_idx) {
                                                if grp.profiles.len() > 1 {
                                                    let removed = grp.profiles.remove(di);
                                                    removed_info = Some((removed.id, removed.name));
                                                    if self.cfg.selected_profile_idx >= grp.profiles.len() {
                                                        self.cfg.selected_profile_idx = grp.profiles.len() - 1;
                                                    }
                                                }
                                            }
                                            if let Some((rid, rname)) = removed_info {
                                                for s in &mut self.cfg.routing_sets {
                                                    s.profile_ids.retain(|x| x != &rid);
                                                }
                                                left_save_note = Some(format!("Deleted sub-profile '{rname}'."));
                                            }
                                        }
                                    });
                                },
                            );
                        });

                        if let Some(note) = left_save_note {
                            self.save_now(&note);
                        }
                    },
                );

                ui.add_space(col_gap);

                // ---------------------------------------------------------------------
                // RIGHT WORKSPACE: FULL-HEIGHT PROFILE INSPECTOR
                // ---------------------------------------------------------------------
                                let saved_engines_snap = self.cfg.saved_engines.clone();
                let global_llama_dir = self.cfg.llama_dir.clone();
                let saved_engines_for_colors = self.cfg.saved_engines.clone();

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
                                        let name = self.loaded_label.as_deref().unwrap_or("Active Engine");
                                        (format!("[LOADED IN VRAM] {name}"), egui::Color32::from_rgb(52, 211, 153))
                                    } else {
                                        ("[UNLOADED] 0 MB VRAM Used (Safe for Gaming)".to_string(), egui::Color32::from_rgb(251, 191, 36))
                                    };
                                    ui.add(egui::Label::new(egui::RichText::new(vram_txt).strong().color(vram_col)).selectable(false));
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
                                            egui::RichText::new(format!("> LAUNCH SET: {active_set_name}")).strong().color(pal.accent_text),
                                        )
                                        .fill(pal.accent);
                                        if ui.add_sized([210.0, 32.0], load_set_btn).clicked() {
                                            self.sync_backend_exes();
                                            if let Some(set) = self.cfg.routing_sets.get(self.cfg.selected_set_idx).cloned() {
                                                match launch_routing_set(
                                                    &self.cfg,
                                                    &set,
                                                    Arc::clone(&self.llama_console),
                                                    Arc::clone(&self.error_logs),
                                                    ui.ctx().clone(),
                                                    llama_vp_id(),
                                                ) {
                                                    Ok(()) => {
                                                        self.loaded_label = Some(format!("Routing Set: {} [{}]", set.name, set.mode));
                                                        procs.llama = true;
                                                        self.set_procs(procs);
                                                        self.status_msg = format!("Launched Routing Set '{}' ({} linked models).", set.name, set.profile_ids.len());
                                                    }
                                                    Err(e) => self.push_error(e),
                                                }
                                            }
                                        }

                                        let direct_btn = egui::Button::new(
                                            egui::RichText::new("> Load Single Profile").strong().color(pal.btn_idle_text),
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
                                                    ui.ctx().clone(),
                                                    llama_vp_id(),
                                                ) {
                                                    Ok(()) => {
                                                        self.loaded_label = Some(format!("{gtitle} -> {} [{}]", prof.name, prof.engine_badge_name()));
                                                        procs.llama = true;
                                                        self.set_procs(procs);
                                                        self.status_msg = format!("Loaded '{gtitle}' ({}) via {}.", prof.name, prof.effective_engine_exe());
                                                    }
                                                    Err(e) => self.push_error(e),
                                                }
                                            }
                                        }

                                        let unload_btn = egui::Button::new(egui::RichText::new("X UNLOAD").strong().color(egui::Color32::WHITE))
                                            .fill(egui::Color32::from_rgb(153, 27, 27));
                                        if ui.add_sized([100.0, 32.0], unload_btn).clicked() {
                                            let exes = collect_backend_exes(&self.cfg);
                                            stop_all_backends(&exes);
                                            self.loaded_label = None;
                                            procs.llama = false;
                                            self.set_procs(procs);
                                            self.status_msg = "Engine unloaded. VRAM freed!".to_string();
                                        }

                                        let save_fill = if edit_mode { pal.edit_hl } else { egui::Color32::from_rgb(16, 125, 78) };
                                        let save_btn = egui::Button::new(egui::RichText::new("Save Changes").strong().color(contrast_on(save_fill))).fill(save_fill);
                                        if ui.add_sized([112.0, 32.0], save_btn).clicked() {
                                            trigger_save_msg = Some("Saved all edits, routing sets, and flags.".to_string());
                                        }

                                        if ui.add_enabled(has_unsaved, egui::Button::new("Revert").min_size(egui::vec2(68.0, 32.0))).clicked() {
                                            revert_requested = true;
                                        }
                                        if has_unsaved { ui.label(egui::RichText::new("Unsaved / Temp").size(11.0).strong().color(egui::Color32::from_rgb(250, 204, 21))); }
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
                                                    ui.add(egui::Label::new(
                                                        egui::RichText::new("EDIT MODEL FAMILY, SUB-PROFILE, HELP KEY & BOUND ENGINE")
                                                            .size(11.5)
                                                            .strong()
                                                            .color(pal.edit_hl),
                                                    ).selectable(false));

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
                                                                    if ui.selectable_label(prof.engine_dir_override.is_empty() && prof.engine_label.is_empty(), "Use Global Default").clicked() {
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
                                                                            if eng.engine_kind == "custom" && prof.single.extra_args.trim().is_empty() && !eng.default_args.trim().is_empty() {
                                                                                prof.single.extra_args = eng.default_args.clone();
                                                                            }
                                                                            trigger_save_msg = Some(format!("Bound sub-profile to engine '{}'.", eng.label));
                                                                        }
                                                                    }
                                                                });

                                                            ui.label("Engine Label:");
                                                            pasteable_singleline(ui, &mut prof.engine_label, 130.0, "Global Default");

                                                            let cur_mode = if prof.effective_engine_kind() == "custom" { "custom" } else { "llama_cpp" };
                                                            ui.label("CLI Style:");
                                                            egui::ComboBox::from_id_salt("prof_eng_kind").selected_text(cur_mode).width(88.0).show_ui(ui, |ui| {
                                                                ui.selectable_value(&mut prof.engine_kind, "llama_cpp".to_string(), "llama_cpp");
                                                                ui.selectable_value(&mut prof.engine_kind, "custom".to_string(), "custom");
                                                            });
                                                        });
                                                        ui.horizontal_wrapped(|ui| {
                                                            ui.label("Exe:");
                                                            pasteable_singleline(ui, &mut prof.engine_exe_override, 165.0, "llama-server.exe");
                                                            if ui.button("Pick .exe").clicked() {
                                                                let start_dir = if prof.engine_dir_override.trim().is_empty() { global_llama_dir.as_str() } else { prof.engine_dir_override.as_str() };
                                                                let mut dlg = FileDialog::new(); if cfg!(windows) { dlg = dlg.add_filter("Executable", &["exe"]); }
                                                                if Path::new(start_dir).exists() { dlg = dlg.set_directory(start_dir); }
                                                                if let Some(p) = dlg.pick_file() {
                                                                    if let Some(par) = p.parent() { prof.engine_dir_override = par.to_string_lossy().to_string(); }
                                                                    if let Some(fname) = p.file_name() { prof.engine_exe_override = fname.to_string_lossy().to_string(); }
                                                                    if prof.engine_label.trim().is_empty() {
                                                                        if let Some(stem) = p.file_stem() { prof.engine_label = stem.to_string_lossy().to_string(); }
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
                                            ui.add(egui::Label::new(
                                                egui::RichText::new(format!("{} — {}", grp.model_title, prof.name))
                                                    .size(15.0)
                                                    .strong()
                                                    .color(pal.text),
                                            ).selectable(false));
                                            let bound_dir = prof.effective_engine_dir(&global_llama_dir);
                                            let ecol = engine_badge_color_for(prof.engine_badge_name(), &saved_engines_for_colors);
                                            ui.add(egui::Label::new(
                                                egui::RichText::new(format!(
                                                    "Engine [{}]: {} -> {}",
                                                    prof.engine_badge_name(),
                                                    prof.effective_engine_exe(),
                                                    bound_dir
                                                ))
                                                .size(11.0)
                                                .color(ecol),
                                            ).selectable(false));
                                            ui.add_space(4.0);
                                            egui::Frame::new()
                                                .fill(pal.item_bg)
                                                .stroke(egui::Stroke::new(1.0, pal.item_border))
                                                .corner_radius(egui::CornerRadius::same(10))
                                                .inner_margin(egui::Margin::same(8))
                                                .show(ui, |ui| {
                                                    ui.label(egui::RichText::new(&prof.packed_description).size(12.0).color(pal.text));
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
                                                        .color(if simple_sel { pal.accent_text } else { pal.btn_idle_text }),
                                                )
                                                .fill(if simple_sel { pal.accent } else { pal.btn_idle });
                                                if ui.add(s_btn).clicked() {
                                                    prof.use_manual_flags = false;
                                                }

                                                let a_btn = egui::Button::new(
                                                    egui::RichText::new("Advanced (Manual Flags)")
                                                        .strong()
                                                        .color(if adv_sel { pal.accent_text } else { pal.btn_idle_text }),
                                                )
                                                .fill(if adv_sel { pal.accent } else { pal.btn_idle });
                                                if ui.add(a_btn).clicked() {
                                                    if prof.manual_flags.trim().is_empty() {
                                                        prof.manual_flags = prof.generate_manual_template();
                                                    }
                                                    prof.use_manual_flags = true;
                                                }

                                                if prof.use_manual_flags {
                                                    ui.label(egui::RichText::new("Using manual flags").size(11.0).color(pal.muted));
                                                }
                                            });
                                            ui.add_space(4.0);

                                            if prof.use_manual_flags {
                                                render_advanced_inspector(
                                                    ui,
                                                    prof,
                                                    edit_mode,
                                                    self.show_plain_hints,
                                                    pal,
                                                    &models_dir,
                                                    &mut self.scanned_models,
                                                    &mut self.gguf_cache,
                                                    &mut trigger_save_msg,
                                                );
                                            } else {
                                                let sum_col = if edit_mode { pal.edit_hl } else { pal.accent };
                                                ui.add(egui::Label::new(
                                                    egui::RichText::new(format!("Active Flags: {}", prof.single.short_summary()))
                                                        .size(11.5)
                                                        .color(sum_col),
                                                ).selectable(false));

                                                let mut port_copy = prof.port;
                                                render_spec_inspector(
                                                    ui,
                                                    &mut prof.single,
                                                    edit_mode,
                                                    self.show_plain_hints,
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
                ui.add_space(8.0);
                themed_card(pal).show(ui, |ui| {
                    let full_log = self
                        .error_logs
                        .lock()
                        .map(|l| if l.is_empty() { "No errors logged yet.".to_string() } else { l.join("\n\n") })
                        .unwrap_or_default();

                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new("ERROR & DIAGNOSTICS LOG (SELECTABLE)").strong().color(egui::Color32::from_rgb(248, 113, 113))).selectable(false));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() { self.show_error_log = false; }
                            if ui.button("Clear Log").clicked() {
                                if let Ok(mut l) = self.error_logs.lock() { l.clear(); }
                            }
                            if ui.button("Notepad").clicked() {
                                runtime::open_text_in_notepad("error_log.txt", &full_log);
                            }
                            let copy_btn = egui::Button::new(egui::RichText::new("Copy Error Log").strong().color(pal.accent_text)).fill(pal.accent);
                            if ui.add(copy_btn).clicked() && runtime::copy_to_clipboard(&full_log) {
                                self.status_msg = "Copied error log to clipboard!".to_string();
                            }
                        });
                    });

                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("cc_err_scroll").max_height(88.0).show(ui, |ui| {
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
    let cfg = StackConfig::load_with_hooks(patch.default_config_fn, patch.legacy_migrator_fn, patch.post_load_fn);
    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1440.0, 860.0]).with_min_inner_size([1120.0, 660.0]);
    if let Some(icon) = load_window_icon(&cfg.favicon_path, patch.embedded_icon_bytes) { viewport = viewport.with_icon(icon); }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Glow, ..Default::default() };
    let (init_mode, init_accent) = (cfg.theme_mode, cfg.accent_rgb);
    let title = format!("{} {}", effective_app_title(&cfg, &patch), patch.effective_version());
    eframe::run_native(&title, options, Box::new(move |cc| {
        apply_app_theme(&cc.egui_ctx, init_mode, init_accent, false);
        Ok(Box::new(ControlCenterApp::new(cc.egui_ctx.clone(), patch)))
    }))
}
