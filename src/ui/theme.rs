use crate::config::{EnginePreset, ThemeMode};
use crate::runtime;
use eframe::egui;

#[derive(Clone, Copy)]
pub(crate) struct UiPalette {
    pub canvas: egui::Color32,
    pub card_bg: egui::Color32,
    pub card_border: egui::Color32,
    pub item_bg: egui::Color32,
    pub item_border: egui::Color32,
    pub item_sel_bg: egui::Color32,
    pub item_sel_border: egui::Color32,
    pub sub_sel_bg: egui::Color32,
    pub sub_sel_border: egui::Color32,
    pub code_bg: egui::Color32,
    pub text: egui::Color32,
    pub muted: egui::Color32,
    pub accent: egui::Color32,
    pub accent_text: egui::Color32,
    pub btn_idle: egui::Color32,
    pub btn_idle_text: egui::Color32,
    pub edit_hl: egui::Color32,
}

pub(crate) const ENGINE_COLOR_PALETTE: [[u8; 3]; 8] = [
    [59, 130, 246],  // Blue
    [234, 120, 48],  // Orange
    [16, 185, 129],  // Emerald
    [168, 85, 247],  // Purple
    [6, 182, 212],   // Cyan
    [244, 63, 94],   // Rose
    [234, 179, 8],   // Gold
    [129, 140, 248], // Indigo
];

pub(crate) fn contrast_on(c: egui::Color32) -> egui::Color32 {
    let luma = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    if luma > 155.0 {
        egui::Color32::from_rgb(24, 22, 19)
    } else {
        egui::Color32::WHITE
    }
}

pub(crate) fn blend_color(base: egui::Color32, tint: egui::Color32, factor: f32) -> egui::Color32 {
    let f = factor.clamp(0.0, 1.0);
    let r = (base.r() as f32 * (1.0 - f) + tint.r() as f32 * f).round() as u8;
    let g = (base.g() as f32 * (1.0 - f) + tint.g() as f32 * f).round() as u8;
    let b = (base.b() as f32 * (1.0 - f) + tint.b() as f32 * f).round() as u8;
    egui::Color32::from_rgb(r, g, b)
}

pub(crate) fn palette_for(mode: ThemeMode, accent_rgb: [u8; 3], edit_mode: bool) -> UiPalette {
    let accent = egui::Color32::from_rgb(accent_rgb[0], accent_rgb[1], accent_rgb[2]);
    let accent_text = contrast_on(accent);
    let w = egui::Color32::WHITE;
    let em = egui::Color32::from_rgb(16, 185, 129);
    match mode {
        ThemeMode::OledBlack if edit_mode => UiPalette {
            canvas: egui::Color32::BLACK,
            card_bg: egui::Color32::from_rgb(14, 14, 16),
            card_border: w,
            item_bg: egui::Color32::from_rgb(20, 20, 24),
            item_border: egui::Color32::from_rgb(125, 125, 135),
            item_sel_bg: egui::Color32::from_rgb(42, 44, 52),
            item_sel_border: w,
            sub_sel_bg: egui::Color32::from_rgb(42, 44, 52),
            sub_sel_border: w,
            code_bg: egui::Color32::from_rgb(9, 9, 11),
            text: w,
            muted: egui::Color32::from_rgb(190, 194, 204),
            accent,
            accent_text,
            btn_idle: egui::Color32::from_rgb(32, 34, 40),
            btn_idle_text: w,
            edit_hl: w,
        },
        ThemeMode::OledBlack => UiPalette {
            canvas: egui::Color32::BLACK,
            card_bg: egui::Color32::from_rgb(12, 12, 17),
            card_border: egui::Color32::from_rgb(36, 38, 52),
            item_bg: egui::Color32::from_rgb(18, 18, 25),
            item_border: egui::Color32::from_rgb(36, 36, 48),
            item_sel_bg: blend_color(egui::Color32::from_rgb(14, 16, 24), accent, 0.28),
            item_sel_border: accent,
            sub_sel_bg: egui::Color32::from_rgb(20, 56, 48),
            sub_sel_border: em,
            code_bg: egui::Color32::from_rgb(8, 9, 14),
            text: egui::Color32::from_rgb(236, 238, 244),
            muted: egui::Color32::from_rgb(154, 162, 178),
            accent,
            accent_text,
            btn_idle: egui::Color32::from_rgb(28, 30, 42),
            btn_idle_text: egui::Color32::from_rgb(230, 234, 242),
            edit_hl: w,
        },
        ThemeMode::Grey if edit_mode => UiPalette {
            canvas: egui::Color32::from_rgb(30, 32, 36),
            card_bg: egui::Color32::from_rgb(42, 44, 50),
            card_border: w,
            item_bg: egui::Color32::from_rgb(50, 53, 60),
            item_border: egui::Color32::from_rgb(145, 149, 160),
            item_sel_bg: egui::Color32::from_rgb(68, 72, 82),
            item_sel_border: w,
            sub_sel_bg: egui::Color32::from_rgb(68, 72, 82),
            sub_sel_border: w,
            code_bg: egui::Color32::from_rgb(24, 26, 30),
            text: w,
            muted: egui::Color32::from_rgb(194, 198, 208),
            accent,
            accent_text,
            btn_idle: egui::Color32::from_rgb(58, 62, 70),
            btn_idle_text: w,
            edit_hl: w,
        },
        ThemeMode::Grey => UiPalette {
            canvas: egui::Color32::from_rgb(30, 33, 39),
            card_bg: egui::Color32::from_rgb(40, 44, 52),
            card_border: egui::Color32::from_rgb(64, 70, 82),
            item_bg: egui::Color32::from_rgb(48, 53, 62),
            item_border: egui::Color32::from_rgb(70, 76, 90),
            item_sel_bg: blend_color(egui::Color32::from_rgb(44, 49, 58), accent, 0.28),
            item_sel_border: accent,
            sub_sel_bg: blend_color(egui::Color32::from_rgb(40, 50, 48), em, 0.24),
            sub_sel_border: em,
            code_bg: egui::Color32::from_rgb(24, 27, 32),
            text: egui::Color32::from_rgb(234, 237, 242),
            muted: egui::Color32::from_rgb(162, 170, 184),
            accent,
            accent_text,
            btn_idle: egui::Color32::from_rgb(54, 59, 70),
            btn_idle_text: egui::Color32::from_rgb(236, 240, 246),
            edit_hl: w,
        },
        // Warm matte paper Light theme (low glare, no pure #FFFFFF surfaces)
        ThemeMode::Light if edit_mode => {
            let ink = egui::Color32::from_rgb(34, 31, 27);
            UiPalette {
                canvas: egui::Color32::from_rgb(222, 217, 208),
                card_bg: egui::Color32::from_rgb(235, 231, 223),
                card_border: ink,
                item_bg: egui::Color32::from_rgb(229, 224, 215),
                item_border: egui::Color32::from_rgb(144, 137, 126),
                item_sel_bg: egui::Color32::from_rgb(211, 205, 194),
                item_sel_border: ink,
                sub_sel_bg: egui::Color32::from_rgb(211, 205, 194),
                sub_sel_border: ink,
                code_bg: egui::Color32::from_rgb(218, 212, 202),
                text: ink,
                muted: egui::Color32::from_rgb(95, 89, 81),
                accent,
                accent_text,
                btn_idle: egui::Color32::from_rgb(212, 206, 196),
                btn_idle_text: ink,
                edit_hl: ink,
            }
        }
        ThemeMode::Light => {
            let paper = egui::Color32::from_rgb(236, 232, 224);
            let em_warm = egui::Color32::from_rgb(15, 148, 102);
            let ink = egui::Color32::from_rgb(36, 33, 29);
            UiPalette {
                canvas: egui::Color32::from_rgb(224, 219, 210),
                card_bg: paper,
                card_border: egui::Color32::from_rgb(192, 185, 173),
                item_bg: egui::Color32::from_rgb(230, 225, 216),
                item_border: egui::Color32::from_rgb(198, 191, 179),
                item_sel_bg: blend_color(paper, accent, 0.16),
                item_sel_border: accent,
                sub_sel_bg: blend_color(paper, em_warm, 0.16),
                sub_sel_border: em_warm,
                code_bg: egui::Color32::from_rgb(220, 214, 204),
                text: ink,
                muted: egui::Color32::from_rgb(104, 98, 89),
                accent,
                accent_text,
                btn_idle: egui::Color32::from_rgb(214, 208, 198),
                btn_idle_text: egui::Color32::from_rgb(38, 35, 31),
                edit_hl: ink,
            }
        }
    }
}

pub(crate) fn default_engine_rgb(label_or_tag: &str) -> [u8; 3] {
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
    let hash = low
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    ENGINE_COLOR_PALETTE[1 + (hash % (ENGINE_COLOR_PALETTE.len() - 1))]
}

pub(crate) fn engine_badge_color_for(
    label_or_tag: &str,
    saved_engines: &[EnginePreset],
) -> egui::Color32 {
    let trimmed = label_or_tag.trim();
    if !trimmed.is_empty() {
        let low = trimmed.to_ascii_lowercase();
        if let Some(eng) = saved_engines
            .iter()
            .find(|e| e.label.trim().eq_ignore_ascii_case(trimmed))
            .or_else(|| {
                saved_engines.iter().find(|e| {
                    let el = e.label.trim().to_ascii_lowercase();
                    !el.is_empty() && (el.contains(&low) || low.contains(&el))
                })
            })
        {
            if let Some([r, g, b]) = eng.color_rgb {
                return egui::Color32::from_rgb(r, g, b);
            }
        }
    }
    let [r, g, b] = default_engine_rgb(label_or_tag);
    egui::Color32::from_rgb(r, g, b)
}

pub(crate) fn apply_app_theme(
    ctx: &egui::Context,
    mode: ThemeMode,
    accent_rgb: [u8; 3],
    edit_mode: bool,
) {
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

pub(crate) fn themed_card(pal: UiPalette) -> egui::Frame {
    egui::Frame::new()
        .fill(pal.card_bg)
        .stroke(egui::Stroke::new(1.0, pal.card_border))
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(12))
}

pub(crate) fn pill_switch(
    ui: &mut egui::Ui,
    value: &mut bool,
    on_text: &str,
    off_text: &str,
    edit_mode: bool,
    pal: UiPalette,
    tip: &str,
) -> bool {
    let label = if *value { on_text } else { off_text };
    let fill = if *value {
        if edit_mode {
            pal.edit_hl
        } else {
            egui::Color32::from_rgb(16, 125, 78)
        }
    } else {
        pal.btn_idle
    };
    let text_color = if *value {
        contrast_on(fill)
    } else {
        pal.btn_idle_text
    };
    let btn = egui::Button::new(egui::RichText::new(label).strong().color(text_color))
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(14));
    let resp = ui.add(btn);
    let resp = if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    };
    if resp.clicked() {
        *value = !*value;
        return true;
    }
    false
}

pub(crate) fn plain_help_row(
    ui: &mut egui::Ui,
    show_hints: bool,
    pal: UiPalette,
    tags: &[(&str, egui::Color32)],
    text: &str,
) {
    if !show_hints {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (tag, col) in tags {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("[{tag}]"))
                        .size(10.5)
                        .strong()
                        .color(*col),
                )
                .selectable(false),
            );
        }
        ui.add(
            egui::Label::new(egui::RichText::new(text).size(11.0).color(pal.muted))
                .selectable(false),
        );
    });
}

pub(crate) fn pasteable_singleline(
    ui: &mut egui::Ui,
    value: &mut String,
    width: f32,
    hint: &str,
) -> egui::Response {
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