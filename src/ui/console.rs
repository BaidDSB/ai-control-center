use super::theme::pasteable_singleline;
use crate::runtime::{self, ProcStatus, SharedConsole, UiNotifier, UiNotifyTarget};
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) fn llama_vp_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("ai_cc_llama_console_vp")
}

pub(crate) fn owu_vp_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("ai_cc_owu_console_vp")
}

pub(crate) fn console_notifier(ctx: egui::Context, vp_id: egui::ViewportId) -> UiNotifier {
    Arc::new(move |target| match target {
        UiNotifyTarget::Root => ctx.request_repaint_of(egui::ViewportId::ROOT),
        UiNotifyTarget::Console => ctx.request_repaint_of(vp_id),
        UiNotifyTarget::Both => {
            ctx.request_repaint_of(vp_id);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_console_viewport(
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

            let copy_id = egui::Id::from(vp_id).with("copied_at");
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
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("[{st_txt}]"))
                                .strong()
                                .color(st_col),
                        )
                        .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(egui::RichText::new(title).strong().color(accent))
                            .selectable(false),
                    );
                    ui.separator();
                    pasteable_singleline(ui, &mut guard.filter, 150.0, "Filter logs...");
                    ui.checkbox(&mut guard.auto_scroll, "Auto-scroll");

                    if recently_copied {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new("Copied!")
                                    .strong()
                                    .color(egui::Color32::from_rgb(52, 211, 153)),
                            )
                            .selectable(false),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Hide").clicked() {
                            hide_req = true;
                        }
                        if ui.button("Clear").clicked() {
                            clear_req = true;
                        }
                        if ui.button("Notepad").clicked() {
                            notepad_req = true;
                        }
                        let copy_all_btn = egui::Button::new(
                            egui::RichText::new("Copy All")
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(37, 99, 235));
                        if ui.add(copy_all_btn).clicked() {
                            copy_all_req = true;
                        }
                        let copy_last_btn = egui::Button::new(
                            egui::RichText::new("Copy Last 40")
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(16, 125, 78));
                        if ui.add(copy_last_btn).clicked() {
                            copy_last_req = true;
                        }
                    });
                });

                if clear_req {
                    guard.clear();
                }

                let flt = guard.filter.trim().to_ascii_lowercase();
                let list: Vec<String> = if flt.is_empty() {
                    guard.lines.iter().cloned().collect()
                } else {
                    guard
                        .lines
                        .iter()
                        .filter(|l| l.to_ascii_lowercase().contains(&flt))
                        .cloned()
                        .collect()
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
                let fname = if is_llama {
                    "llama_console.txt"
                } else {
                    "owu_console.txt"
                };
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
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "Showing last {} of {} lines (total: {}){} — Closing this window keeps the server running.",
                        filtered_lines.len() - shown_start,
                        filtered_lines.len(),
                        total_lines,
                        if filter_lower.is_empty() { "" } else { " [filtered]" }
                    ))
                    .size(11.0)
                    .color(egui::Color32::from_rgb(138, 146, 162)),
                )
                .selectable(false),
            );
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