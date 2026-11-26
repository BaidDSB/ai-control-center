use super::theme::{pasteable_singleline, pill_switch, plain_help_row, UiPalette};
use crate::config::{probe_gguf_header, scan_gguf_models, GgufInfo, ModelProfile, ModelSpec, RpcFallbackPolicy};
use crate::runtime;
use eframe::egui;
use rfd::FileDialog;
use std::collections::HashMap;

fn hint_tag_colors(is_light: bool) -> (egui::Color32, egui::Color32, egui::Color32, egui::Color32) {
    if is_light {
        (
            egui::Color32::from_rgb(37, 99, 200),
            egui::Color32::from_rgb(14, 124, 78),
            egui::Color32::from_rgb(168, 98, 8),
            egui::Color32::from_rgb(126, 58, 198),
        )
    } else {
        (
            egui::Color32::from_rgb(96, 165, 250),
            egui::Color32::from_rgb(52, 211, 153),
            egui::Color32::from_rgb(251, 191, 36),
            egui::Color32::from_rgb(192, 132, 252),
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_spec_inspector(
    ui: &mut egui::Ui,
    spec: &mut ModelSpec,
    edit_mode: bool,
    show_hints: bool,
    is_light: bool,
    pal: UiPalette,
    models_dir: &str,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    port_val: &mut u16,
    secondary_rpc_ep: Option<&str>,
    rpc_fallback: RpcFallbackPolicy,
    id_salt: &str,
    trigger_save_msg: &mut Option<String>,
) {
    let (c_vram, c_speed, c_qual, c_ram) = hint_tag_colors(is_light);

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
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "GGUF Info (Cached): {:.1} GB | Arch: {} | {} | {}{}",
                    ginfo.file_size_gb, ginfo.arch, layers_txt, ctx_txt, exp_txt
                ))
                .size(11.0)
                .color(hdr_col),
            )
            .selectable(false),
        );
    }

    ui.add_space(4.0);

    // 1. HARDWARE & VRAM SPLIT
    egui::CollapsingHeader::new("1. Hardware, VRAM Split & MoE Cache")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let ngl_tip = "GPU Layers (-ngl): How many transformer blocks sit in GPU VRAM.\nUP: Faster tok/s, uses more VRAM.\nDOWN: Frees VRAM for bigger context.";
                ui.label("GPU Layers (-ngl):").on_hover_text(ngl_tip);
                egui::ComboBox::from_id_salt((id_salt, "ngl"))
                    .selected_text(&spec.ngl_mode)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut spec.ngl_mode, "auto".to_string(), "auto");
                        ui.selectable_value(&mut spec.ngl_mode, "custom".to_string(), "custom");
                    });
                if spec.ngl_mode == "custom" {
                    ui.add(egui::DragValue::new(&mut spec.ngl_layers).range(0..=999))
                        .on_hover_text(ngl_tip);
                }
                ui.separator();
                let fit_tip = "Auto-Fit (-fit): Automatic VRAM balancer.\nON: Protects against OOM.\nOFF: Strict mode — obeys exact -c and -ngl.";
                ui.label("Auto-Fit (-fit):").on_hover_text(fit_tip);
                egui::ComboBox::from_id_salt((id_salt, "fit"))
                    .selected_text(&spec.fit_mode)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut spec.fit_mode, "default".to_string(), "default");
                        ui.selectable_value(&mut spec.fit_mode, "on".to_string(), "on (Protect VRAM)");
                        ui.selectable_value(&mut spec.fit_mode, "off".to_string(), "off (Strict)");
                    });
                if spec.fit_mode == "on" {
                    ui.label("Margin (MiB):").on_hover_text(fit_tip);
                    ui.add(egui::DragValue::new(&mut spec.fit_target_mib).range(256..=4096).speed(64))
                        .on_hover_text(fit_tip);
                }
            });
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("VRAM", c_vram), ("Speed", c_speed)],
                "GPU Layers (-ngl): UP = faster tok/s + more VRAM; DOWN = less VRAM + slower RAM compute. Auto-Fit OFF locks exact context.",
            );

            ui.horizontal_wrapped(|ui| {
                let moe_tip = "MoE CPU Offload: Keeps attention/shared layers on GPU while moving routed MoE experts to system RAM.";
                pill_switch(ui, &mut spec.moe_enabled, "MoE Offload: ON", "MoE Offload: OFF", edit_mode, pal, moe_tip);
                if spec.moe_enabled {
                    pill_switch(
                        ui,
                        &mut spec.all_moe_cpu,
                        "All CPU (-cmoe)",
                        "Layer Count",
                        edit_mode,
                        pal,
                        "All MoE on CPU (-cmoe) vs exact --n-cpu-moe count.",
                    );
                    if !spec.all_moe_cpu {
                        ui.label("CPU Layers:").on_hover_text(moe_tip);
                        ui.add(egui::DragValue::new(&mut spec.n_cpu_moe).range(1..=99))
                            .on_hover_text(moe_tip);
                    }
                    let cache_tip = "MoE Expert Cache (--moe-cache-mib): Dynamic VRAM cache for hottest MoE experts (0 = off).";
                    ui.label("Cache (MiB):").on_hover_text(cache_tip);
                    ui.add(egui::DragValue::new(&mut spec.moe_cache_mib).range(0..=6144).speed(64))
                        .on_hover_text(cache_tip);

                    if spec.moe_cache_mib > 0 {
                        let share_tip = "MoE CPU Share (--moe-cache-cpu-share): Fraction of cache misses computed by CPU instead of PCIe upload (0.00 to 1.00).";
                        ui.label("CPU Share:").on_hover_text(share_tip);
                        ui.add(
                            egui::DragValue::new(&mut spec.moe_cache_cpu_share)
                                .range(0.0..=1.0)
                                .speed(0.05)
                                .fixed_decimals(2),
                        )
                        .on_hover_text(share_tip);
                    }
                }
            });
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("VRAM", c_vram), ("Speed", c_speed)],
                "MoE Offload: --n-cpu-moe UP = more VRAM free (offloads MoE layers to RAM). Expert Cache MiB UP = higher hit rate until VRAM limit.",
            );

            ui.horizontal_wrapped(|ui| {
                ui.label("Dense CPU FFN (-ncffn):");
                ui.add(egui::DragValue::new(&mut spec.n_cpu_ffn).range(0..=99));
                ui.separator();
                pill_switch(
                    ui,
                    &mut spec.disable_cuda_pinned,
                    "No CUDA Pinned: ON",
                    "No CUDA Pinned: OFF",
                    edit_mode,
                    pal,
                    "Sets GGML_CUDA_NO_PINNED=1 for this server process only.",
                );
                ui.separator();
                pill_switch(
                    ui,
                    &mut spec.use_rpc,
                    "RPC: ON",
                    "RPC: OFF",
                    edit_mode,
                    pal,
                    "Pools remote GPU VRAM from the centralized Secondary Node configured in Setup. Auto policy falls back locally in 120ms if the node is off.",
                );
                if spec.use_rpc {
                    let pol_tag = match rpc_fallback {
                        RpcFallbackPolicy::Auto => "Auto",
                        RpcFallbackPolicy::Strict => "Strict",
                    };
                    if let Some(ep) = secondary_rpc_ep {
                        ui.label(
                            egui::RichText::new(format!("-> {ep} ({pol_tag})"))
                                .size(11.5)
                                .color(c_vram),
                        )
                        .on_hover_text("Managed centrally in Edit Mode -> Setup -> Secondary Node (LAN / RPC).");
                    } else {
                        ui.label(
                            egui::RichText::new("No Secondary Node enabled (set in Setup)")
                                .size(11.0)
                                .color(c_qual),
                        )
                        .on_hover_text("Enable Edit Mode -> Setup -> Secondary Node (LAN / RPC) to set Host/IP and RPC Port.");
                    }
                }
            });
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("VRAM", c_vram), ("RAM", c_ram)],
                "Dense FFN UP = frees VRAM on dense models. No CUDA Pinned ON = prevents Shared GPU spikes on Windows. RPC ON = pools remote GPU VRAM over LAN.",
            );
        });

    // 2. CONTEXT & KV CACHE COMPRESSION
    egui::CollapsingHeader::new("2. Context & Memory Compression (KV Cache)")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let ctx_tip = "Context Size (-c): Total conversation memory in tokens. Also replaces {ctx} in custom engines like RustingStrata.";
                ui.label("Context (-c):").on_hover_text(ctx_tip);
                egui::ComboBox::from_id_salt((id_salt, "ctx"))
                    .selected_text(format!("Preset ({})", spec.ctx_size))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for c in [8192, 16384, 32768, 65536, 131072, 262144] {
                            ui.selectable_value(&mut spec.ctx_size, c, c.to_string());
                        }
                    });
                ui.add(egui::DragValue::new(&mut spec.ctx_size).range(2048..=1_048_576).speed(1024))
                    .on_hover_text(ctx_tip);

                let kv_tip = "KV Cache Quantization (-ctk / -ctv): Higher (f16/q8_0) = sharper recall + more VRAM; Lower (q4_0/turbo3) = big VRAM savings.";
                ui.label("KV K (-ctk):").on_hover_text(kv_tip);
                egui::ComboBox::from_id_salt((id_salt, "ctk"))
                    .selected_text(&spec.cache_type_k)
                    .width(78.0)
                    .show_ui(ui, |ui| {
                        for k in ["f16", "bf16", "f8_e4m3", "q8_0", "q4_0", "iq4_nl", "turbo4", "turbo3", "turbo2"] {
                            ui.selectable_value(&mut spec.cache_type_k, k.to_string(), k);
                        }
                    });
                ui.label("KV V (-ctv):").on_hover_text(kv_tip);
                egui::ComboBox::from_id_salt((id_salt, "ctv"))
                    .selected_text(&spec.cache_type_v)
                    .width(78.0)
                    .show_ui(ui, |ui| {
                        for v in ["f16", "bf16", "f8_e4m3", "q8_0", "q4_0", "iq4_nl", "turbo4", "turbo3", "turbo2"] {
                            ui.selectable_value(&mut spec.cache_type_v, v.to_string(), v);
                        }
                    });
            });
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("VRAM", c_vram), ("Speed", c_speed), ("Quality", c_qual)],
                "Context UP = longer memory + higher VRAM; DOWN = frees VRAM for speed. Use turbo3 on TurboQuant fork for long dense/Gemma 4 context.",
            );

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
    egui::CollapsingHeader::new("3. Thinking, Speculative/MTP & Engine Throughput")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if edit_mode {
                    ui.label("Reasoning Family:");
                    egui::ComboBox::from_id_salt((id_salt, "rfam"))
                        .selected_text(&spec.reasoning_family)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut spec.reasoning_family, "none".to_string(), "none (Instruct)");
                            ui.selectable_value(&mut spec.reasoning_family, "binary".to_string(), "binary (on/off)");
                            ui.selectable_value(&mut spec.reasoning_family, "effort3".to_string(), "effort3 (low/med/xhigh)");
                        });
                }

                match spec.reasoning_family.as_str() {
                    "effort3" => {
                        ui.label("Effort:");
                        egui::ComboBox::from_id_salt((id_salt, "eff3"))
                            .selected_text(&spec.reasoning_mode)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut spec.reasoning_mode, "xhigh".to_string(), "xhigh (Deepest)");
                                ui.selectable_value(&mut spec.reasoning_mode, "medium".to_string(), "medium (Balanced)");
                                ui.selectable_value(&mut spec.reasoning_mode, "low".to_string(), "low (Fast)");
                                ui.selectable_value(&mut spec.reasoning_mode, "off".to_string(), "off (No Thinking)");
                            });
                    }
                    "binary" => {
                        ui.label("Thinking:");
                        egui::ComboBox::from_id_salt((id_salt, "bin"))
                            .selected_text(&spec.reasoning_mode)
                            .show_ui(ui, |ui| {
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
                    egui::ComboBox::from_id_salt((id_salt, "rfmt"))
                        .selected_text(&spec.reasoning_format)
                        .width(85.0)
                        .show_ui(ui, |ui| {
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
                egui::ComboBox::from_id_salt((id_salt, "spectype"))
                    .selected_text(&cur_spec)
                    .width(115.0)
                    .show_ui(ui, |ui| {
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
                    ui.add(
                        egui::DragValue::new(&mut spec.mtp_draft_p_min)
                            .range(0.0..=0.99)
                            .speed(0.05)
                            .fixed_decimals(2),
                    );
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
                egui::ComboBox::from_id_salt((id_salt, "fa"))
                    .selected_text(&spec.flash_attn)
                    .width(62.0)
                    .show_ui(ui, |ui| {
                        for f in ["on", "off", "auto"] {
                            ui.selectable_value(&mut spec.flash_attn, f.to_string(), f);
                        }
                    });
                ui.label("Load Mode (-lm):");
                egui::ComboBox::from_id_salt((id_salt, "lm"))
                    .selected_text(&spec.load_mode)
                    .width(68.0)
                    .show_ui(ui, |ui| {
                        for m in ["auto", "none", "mmap", "mlock", "dio"] {
                            ui.selectable_value(&mut spec.load_mode, m.to_string(), m);
                        }
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
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("Speed", c_speed), ("Quality", c_qual)],
                "For custom engines (RustingStrata), put the full flags in Extra / Custom Flags using {model}, {ctx}, and {addr}.",
            );
        });

    // 4. OPTIONAL SAMPLERS & DRY
    egui::CollapsingHeader::new("4. Samplers & DRY (Optional - Open WebUI Overrides When Set)")
        .default_open(false)
        .show(ui, |ui| {
            plain_help_row(
                ui,
                true,
                pal,
                &[("Quality", c_qual)],
                "Temp UP = creative, DOWN = factual. Pres/Rep Pen UP = avoids repetition (keep Pres=0 in Thinking mode). DRY breaks phrase loops in long stories.",
            );
            ui.horizontal_wrapped(|ui| {
                pill_switch(
                    ui,
                    &mut spec.use_custom_sampling,
                    "Samplers: ON",
                    "Samplers: Default",
                    edit_mode,
                    pal,
                    "ON sends explicit sampler flags; OFF lets server/OWU decide.",
                );
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
pub(crate) fn render_advanced_inspector(
    ui: &mut egui::Ui,
    prof: &mut ModelProfile,
    edit_mode: bool,
    show_hints: bool,
    is_light: bool,
    pal: UiPalette,
    models_dir: &str,
    scanned_models: &mut Vec<String>,
    gguf_cache: &mut HashMap<String, GgufInfo>,
    trigger_save_msg: &mut Option<String>,
) {
    let (c_vram, _c_speed, c_qual, _c_ram) = hint_tag_colors(is_light);

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
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "GGUF Info (Cached): {:.1} GB | Arch: {} | {} | {}{}",
                    ginfo.file_size_gb, ginfo.arch, layers_txt, ctx_txt, exp_txt
                ))
                .size(11.0)
                .color(hdr_col),
            )
            .selectable(false),
        );
    }

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Manual CLI Flags (Raw Override):").strong().color(pal.text));
        if ui
            .button("Pull From Simple")
            .on_hover_text("Overwrite box with flags generated from Simple Mode controls.")
            .clicked()
        {
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
        pill_switch(
            ui,
            &mut prof.single.disable_cuda_pinned,
            "No CUDA Pinned: ON",
            "No CUDA Pinned: OFF",
            edit_mode,
            pal,
            "Sets GGML_CUDA_NO_PINNED=1 for this run.",
        );
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
        .stroke(egui::Stroke::new(
            1.0,
            if edit_mode { pal.edit_hl } else { pal.card_border },
        ))
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
            plain_help_row(
                ui,
                show_hints,
                pal,
                &[("Samplers", c_qual)],
                "If Samplers is ON and you did not manually type --temp above, these values are appended automatically.",
            );
            ui.horizontal_wrapped(|ui| {
                pill_switch(
                    ui,
                    &mut prof.single.use_custom_sampling,
                    "Samplers: ON",
                    "Samplers: Default",
                    edit_mode,
                    pal,
                    "ON appends sampler flags below; OFF uses engine/OWU defaults or whatever you typed manually.",
                );
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