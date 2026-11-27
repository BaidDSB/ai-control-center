use std::sync::Arc;
use super::console::{SharedConsole, SharedErrors, UiNotifier, UiNotifyTarget, spawn_pipe_reader};
use super::state::{
    active_backend_info, clear_tracked_if_pid, normalize_probe_host, probe_tcp_endpoint,
    record_tracked, stop_tracked_backend, stop_tracked_frontend, stop_tracked_mcpo,
    wait_for_port_release, TrackedMeta, TrackedProc, TrackedSlot, RPC_PROBE_TIMEOUT_MS,
};
use crate::config::{ModelProfile, RoutingSet, RpcFallbackPolicy, StackConfig};

fn resolve_profile_rpc_for_launch(
    cfg: &StackConfig,
    profile: &ModelProfile,
) -> (Option<String>, Option<String>) {
    if !profile.single.use_rpc {
        return (None, None);
    }
    let Some(ep) = cfg.effective_rpc_for_profile(&profile.single, None) else {
        return (
            None,
            Some("[RPC] Profile has RPC ON, but Secondary Node is disabled/empty — running locally.".to_string()),
        );
    };
    match cfg.secondary_node.rpc_fallback {
        RpcFallbackPolicy::Strict => (
            Some(ep.clone()),
            Some(format!("[RPC] Strict policy -> pooling remote GPU at {ep}")),
        ),
        RpcFallbackPolicy::Auto => {
            if probe_tcp_endpoint(&ep, RPC_PROBE_TIMEOUT_MS) {
                (
                    Some(ep.clone()),
                    Some(format!("[RPC] Auto probe OK ({ep}) -> pooling remote GPU VRAM")),
                )
            } else {
                (
                    None,
                    Some(format!(
                        "[RPC] Secondary node {ep} unreachable ({RPC_PROBE_TIMEOUT_MS}ms probe) — Auto zero-wait fallback to local GPU/RAM."
                    )),
                )
            }
        }
    }
}

fn resolve_set_rpc_for_launch(
    cfg: &StackConfig,
    linked: &[&ModelProfile],
) -> (Option<bool>, Option<String>) {
    let first_ep = linked
        .iter()
        .find_map(|p| cfg.effective_rpc_for_profile(&p.single, None));
    let Some(ep) = first_ep else {
        if linked.iter().any(|p| p.single.use_rpc) {
            return (
                Some(false),
                Some("[RPC] RPC-enabled profile(s) in set, but Secondary Node is disabled/empty — running locally.".to_string()),
            );
        }
        return (None, None);
    };
    match cfg.secondary_node.rpc_fallback {
        RpcFallbackPolicy::Strict => (
            Some(true),
            Some(format!("[RPC] Strict policy -> pooling remote GPU at {ep}")),
        ),
        RpcFallbackPolicy::Auto => {
            if probe_tcp_endpoint(&ep, RPC_PROBE_TIMEOUT_MS) {
                (
                    Some(true),
                    Some(format!("[RPC] Auto probe OK ({ep}) -> RPC enabled for marked profiles")),
                )
            } else {
                (
                    Some(false),
                    Some(format!(
                        "[RPC] Secondary node {ep} unreachable ({RPC_PROBE_TIMEOUT_MS}ms probe) — Auto zero-wait fallback to local GPU/RAM."
                    )),
                )
            }
        }
    }
}
use crate::platform::{self, CommandExtPlatform};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

pub fn sync_live_llama_swap_yaml(cfg: &StackConfig) -> Result<bool, String> {
    let Some(info) = active_backend_info() else {
        return Ok(false);
    };
    if info.routing_mode != "llama_swap" || info.routing_set_id.is_empty() {
        return Ok(false);
    }
    if !platform::is_pid_running_as(info.pid, &info.exe_name) {
        return Ok(false);
    }
    let Some(set) = cfg.routing_sets.iter().find(|s| s.id == info.routing_set_id) else {
        return Ok(false);
    };
    if set.mode != "llama_swap" || set.profile_ids.is_empty() {
        return Ok(false);
    }
    let mut linked: Vec<&ModelProfile> = Vec::new();
    for pid in &set.profile_ids {
        let Some(p) = cfg.find_profile(pid) else {
            return Err(format!("linked profile '{pid}' missing"));
        };
        linked.push(p);
    }
    let (rpc_reachable, _) = resolve_set_rpc_for_launch(cfg, &linked);
    for p in &linked {
        let resolved_rpc = cfg.effective_rpc_for_profile(&p.single, rpc_reachable);
        let (cmd_check, _) = p.build_swap_cmd_and_env_with_engines_and_rpc(
            &cfg.models_dir,
            &cfg.llama_dir,
            &cfg.saved_engines,
            resolved_rpc.as_deref(),
        );
        if !cmd_check.contains("${PORT}") {
            return Err(format!("profile '{}' is missing {{port}}/{{addr}}", p.name));
        }
    }
    let yaml = cfg.build_routing_set_yaml_with_rpc_status(set, rpc_reachable);
    fs::write(cfg.resolved_llama_swap_yaml_path(), yaml)
        .map_err(|e| format!("failed writing llama-swap.yaml: {e}"))?;
    Ok(true)
}

pub fn collect_backend_exes(cfg: &StackConfig) -> Vec<String> {
    let mut exes: Vec<String> = vec![
        platform::default_exe_name("llama-swap"),
        platform::default_exe_name("llama-server"),
    ];
    let mut push_unique = |name: &str| {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return;
        }
        if !exes.iter().any(|e| e.eq_ignore_ascii_case(trimmed)) {
            exes.push(trimmed.to_string());
        }
    };
    if let Some(fname) = Path::new(&cfg.llama_swap_exe)
        .file_name()
        .and_then(|s| s.to_str())
    {
        push_unique(fname);
    }
    for eng in &cfg.saved_engines {
        push_unique(&eng.exe_name);
    }
    for grp in &cfg.groups {
        for prof in &grp.profiles {
            push_unique(prof.effective_engine_exe());
        }
    }
    exes
}

#[allow(clippy::too_many_arguments)]
fn spawn_monitored(
    exe: &Path,
    work_dir: &Path,
    args: &[String],
    extra_envs: &[(&str, &str)],
    use_external_cmd: bool,
    header_lines: &[String],
    tag: &'static str,
    slot: TrackedSlot,
    meta: TrackedMeta,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
) -> Result<(), String> {
    let exe_name = exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("engine")
        .to_string();
    let mut cmd = Command::new(exe);
    cmd.current_dir(work_dir).args(args);
    for (k, v) in extra_envs {
        cmd.env(k, v);
    }
    platform::prepare_managed_command(&mut cmd);

    if use_external_cmd {
        let child = cmd
            .new_console()
            .spawn()
            .map_err(|e| format!("Failed starting {exe_name}: {e}"))?;
        let pid = child.id();
        let start_id = platform::get_process_start_id(pid);
        record_tracked(
            slot,
            TrackedProc {
                pid,
                exe_name,
                start_id,
                label: meta.label,
                routing_set_id: meta.routing_set_id,
                routing_mode: meta.routing_mode,
            },
        );
        return Ok(());
    }

    if let Ok(mut c) = console.lock() {
        c.push("================================================================".to_string());
        for line in header_lines {
            c.push(line.clone());
        }
        c.push("================================================================".to_string());
    }

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .no_window()
        .spawn()
        .map_err(|e| format!("Failed starting {exe_name}: {e}"))?;

    let pid = child.id();
    let start_id = platform::get_process_start_id(pid);
    record_tracked(
        slot,
        TrackedProc {
            pid,
            exe_name: exe_name.clone(),
            start_id,
            label: meta.label,
            routing_set_id: meta.routing_set_id,
            routing_mode: meta.routing_mode,
        },
    );

    if let Some(stdout) = child.stdout.take() {
        spawn_pipe_reader(
            stdout,
            "",
            tag,
            Arc::clone(&console),
            Arc::clone(&errors),
            Arc::clone(&notify),
        );
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_pipe_reader(
            stderr,
            "",
            tag,
            Arc::clone(&console),
            Arc::clone(&errors),
            Arc::clone(&notify),
        );
    }

    let wait_console = Arc::clone(&console);
    let wait_notify = Arc::clone(&notify);
    thread::spawn(move || {
        let status = child.wait();
        clear_tracked_if_pid(slot, pid);
        if let Ok(mut c) = wait_console.lock() {
            match status {
                Ok(s) => c.push(format!("[{exe_name} EXITED] {s}")),
                Err(e) => c.push(format!("[{exe_name} WAIT ERROR] {e}")),
            }
        }
        wait_notify(UiNotifyTarget::Both);
    });

    notify(UiNotifyTarget::Console);
    Ok(())
}

pub fn launch_single_profile(
    cfg: &StackConfig,
    profile: &ModelProfile,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
) -> Result<(), String> {
    let engine_kind = profile.effective_engine_kind();
    let engine_dir = profile.effective_engine_dir(&cfg.llama_dir);
    let engine_exe = profile.effective_engine_exe();
    let exe = Path::new(engine_dir).join(engine_exe);

    if !exe.exists() {
        return Err(format!(
            "Engine executable '{engine_exe}' not found at {}",
            exe.display()
        ));
    }
    let model_path = Path::new(&cfg.models_dir).join(&profile.single.model_rel_path);
    if !model_path.exists() {
        return Err(format!("Model file not found: {}", model_path.display()));
    }

    let stopped_prev = stop_tracked_backend();
    let probe_host = normalize_probe_host(&cfg.server_host);
    if stopped_prev {
        wait_for_port_release(probe_host, profile.port, 1500);
    }
    if profile.port > 0 && platform::is_port_open(probe_host, profile.port) {
        return Err(format!(
            "Port {} is already in use by another process outside AI Control Center. Stop that server or change this profile's Direct Port.",
            profile.port
        ));
    }

    let (resolved_rpc, rpc_note) = resolve_profile_rpc_for_launch(cfg, profile);
    let args = profile.build_llama_args_with_rpc(&cfg.models_dir, resolved_rpc.as_deref());
    let disable_pinned = profile.should_disable_cuda_pinned();
    let mut headers = vec![format!("[ENGINE] {engine_kind} -> {}", exe.display())];
    if let Some(note) = rpc_note {
        headers.push(note);
    }
    let mut envs: Vec<(&str, &str)> = Vec::new();
    if disable_pinned {
        headers.push("[ENV] GGML_CUDA_NO_PINNED=1".to_string());
        envs.push(("GGML_CUDA_NO_PINNED", "1"));
    }
    headers.push(format!("[LAUNCH] {engine_exe} {}", args.join(" ")));

    let label = cfg
        .find_profile_with_group(&profile.id)
        .map(|(g, _)| {
            format!(
                "{} -> {} [{}]",
                g.model_title,
                profile.name,
                profile.engine_badge_name()
            )
        })
        .unwrap_or_else(|| format!("{} [{}]", profile.name, profile.engine_badge_name()));

    spawn_monitored(
        &exe,
        Path::new(engine_dir),
        &args,
        &envs,
        cfg.use_external_cmd_windows,
        &headers,
        "ENGINE",
        TrackedSlot::Backend,
        TrackedMeta {
            label,
            routing_set_id: String::new(),
            routing_mode: String::new(),
        },
        console,
        errors,
        notify,
    )
}

pub fn launch_routing_set(
    cfg: &StackConfig,
    set: &RoutingSet,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
) -> Result<(), String> {
    if set.profile_ids.is_empty() {
        return Err(format!(
            "Routing Set '{}' has no linked model profiles.",
            set.name
        ));
    }

    let mut linked: Vec<&ModelProfile> = Vec::new();
    for pid in &set.profile_ids {
        let Some(p) = cfg.find_profile(pid) else {
            return Err(format!("Linked profile '{pid}' no longer exists."));
        };
        let model_path = Path::new(&cfg.models_dir).join(&p.single.model_rel_path);
        if !model_path.exists() {
            return Err(format!(
                "Model file for '{}' not found: {}",
                p.name,
                model_path.display()
            ));
        }
        linked.push(p);
    }

    let stopped_prev = stop_tracked_backend();
    let probe_host = normalize_probe_host(&cfg.server_host);
    if stopped_prev {
        wait_for_port_release(probe_host, cfg.server_port, 1500);
    }
    if cfg.server_port > 0 && platform::is_port_open(probe_host, cfg.server_port) {
        return Err(format!(
            "API port {} is already in use by another process outside AI Control Center. Stop that server or change the API port in Setup.",
            cfg.server_port
        ));
    }

    let meta = TrackedMeta {
        label: format!("Routing Set: {} [{}]", set.name, set.mode),
        routing_set_id: set.id.clone(),
        routing_mode: set.mode.clone(),
    };

    if set.mode == "llama_ini" {
        if linked.iter().any(|p| p.effective_engine_kind() == "custom") {
            return Err("Native models.ini mode only supports llama.cpp profiles. Switch set mode to 'llama_swap' for custom / mixed engines.".to_string());
        }
        let first = linked[0];
        let engine_dir = first.effective_engine_dir(&cfg.llama_dir);
        let engine_exe = first.effective_engine_exe();
        let exe = Path::new(engine_dir).join(engine_exe);
        if !exe.exists() {
            return Err(format!(
                "Engine executable '{engine_exe}' not found at {}",
                exe.display()
            ));
        }
        let (rpc_reachable, rpc_note) = resolve_set_rpc_for_launch(cfg, &linked);
        let ini = cfg.build_routing_set_ini_with_rpc_status(set, rpc_reachable);
        fs::write(cfg.resolved_router_ini_path(), ini)
            .map_err(|e| format!("Failed writing models.ini: {e}"))?;

        let mut args = vec![
            "--models-preset".to_string(),
            cfg.resolved_router_ini_path(),
            "--models-max".to_string(),
            "1".to_string(),
        ];
        if set.no_models_autoload {
            args.push("--no-models-autoload".to_string());
        }
        args.extend([
            "--host".to_string(),
            cfg.server_host.trim().to_string(),
            "--port".to_string(),
            cfg.server_port.to_string(),
        ]);
        let disable_pinned = linked.iter().any(|p| p.should_disable_cuda_pinned());
        let mut envs: Vec<(&str, &str)> = Vec::new();
        if disable_pinned {
            envs.push(("GGML_CUDA_NO_PINNED", "1"));
        }
        let mut headers = vec![format!(
            "[ROUTING SET (models.ini)] {} ({} models)",
            set.name,
            linked.len()
        )];
        if let Some(note) = rpc_note {
            headers.push(note);
        }
        headers.push(format!("[LAUNCH] {engine_exe} {}", args.join(" ")));
        return spawn_monitored(
            &exe,
            Path::new(engine_dir),
            &args,
            &envs,
            cfg.use_external_cmd_windows,
            &headers,
            "ENGINE",
            TrackedSlot::Backend,
            meta,
            console,
            errors,
            notify,
        );
    }

    // Default: Multi-Engine Routing via llama-swap
    let swap_exe = Path::new(&cfg.llama_swap_exe);
    let swap_name = swap_exe
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("llama-swap");
    if !swap_exe.exists() {
        return Err(format!("{swap_name} not found at {}", swap_exe.display()));
    }
    let (rpc_reachable, rpc_note) = resolve_set_rpc_for_launch(cfg, &linked);
    for p in &linked {
        let resolved_rpc = cfg.effective_rpc_for_profile(&p.single, rpc_reachable);
        let (cmd_check, _) = p.build_swap_cmd_and_env_with_engines_and_rpc(
            &cfg.models_dir,
            &cfg.llama_dir,
            &cfg.saved_engines,
            resolved_rpc.as_deref(),
        );
        if !cmd_check.contains("${PORT}") {
            return Err(format!(
                "Profile '{}' uses a custom engine with no launch flags. Set Custom Flags on the profile (or Default Args on the engine) including {{model}} and {{port}}/{{addr}} (e.g. serve --model {{model}} --ctx {{ctx}} --addr {{addr}}).",
                p.name
            ));
        }
    }
    let yaml = cfg.build_routing_set_yaml_with_rpc_status(set, rpc_reachable);
    fs::write(cfg.resolved_llama_swap_yaml_path(), yaml)
        .map_err(|e| format!("Failed writing llama-swap.yaml: {e}"))?;

    let listen = format!("{}:{}", cfg.server_host.trim(), cfg.server_port);
    let args = vec![
        "-config".to_string(),
        cfg.resolved_llama_swap_yaml_path(),
        "-listen".to_string(),
        listen.clone(),
        "-watch-config".to_string(),
    ];
    let work_dir = swap_exe.parent().unwrap_or_else(|| Path::new("."));
    let mut headers = vec![format!(
        "[ROUTING SET (llama-swap)] {} ({} models on http://{}/v1)",
        set.name,
        linked.len(),
        listen
    )];
    if let Some(note) = rpc_note {
        headers.push(note);
    }
    for warn in cfg.detect_rpc_peer_collisions(set) {
        headers.push(format!("[WARN] {warn}"));
    }
    headers.push(format!("[LAUNCH] {swap_name} {}", args.join(" ")));
    spawn_monitored(
        swap_exe,
        work_dir,
        &args,
        &[],
        cfg.use_external_cmd_windows,
        &headers,
        "SWAP",
        TrackedSlot::Backend,
        meta,
        console,
        errors,
        notify,
    )
}

pub fn launch_open_webui(
    cfg: &StackConfig,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
) -> Result<(), String> {
    let fe_name = cfg.effective_frontend_name().to_string();
    let exe = cfg.effective_frontend_exe_path();
    let exe_name = cfg.effective_frontend_exe_name();
    if !exe.exists() {
        return Err(format!("{fe_name} executable not found at {}", exe.display()));
    }
    let work_dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(&cfg.owu_scripts_dir).to_path_buf());
    let args = cfg.effective_frontend_args();

    let stopped_prev = stop_tracked_frontend();
    if stopped_prev {
        wait_for_port_release("127.0.0.1", cfg.owu_port, 1500);
    }
    if cfg.owu_port > 0 && platform::is_port_open("127.0.0.1", cfg.owu_port) {
        return Err(format!(
            "Frontend port {} is already in use by another process outside AI Control Center.",
            cfg.owu_port
        ));
    }

    let headers = vec![format!("[LAUNCH] {exe_name} {}", args.join(" "))];
    let envs = [("PYTHONUNBUFFERED", "1"), ("PYTHONIOENCODING", "utf-8")];

    spawn_monitored(
        &exe,
        &work_dir,
        &args,
        &envs,
        cfg.use_external_cmd_windows,
        &headers,
        "FE",
        TrackedSlot::Frontend,
        TrackedMeta {
            label: fe_name,
            ..Default::default()
        },
        console,
        errors,
        notify,
    )
}

pub fn launch_mcpo(cfg: &StackConfig) -> Result<(), String> {
    let mcpo_exe = platform::default_exe_name("mcpo");
    let exe = Path::new(&cfg.owu_scripts_dir).join(&mcpo_exe);
    if !exe.exists() {
        return Err(format!("{mcpo_exe} not found at {}", exe.display()));
    }

    let stopped_prev = stop_tracked_mcpo();
    if stopped_prev {
        wait_for_port_release("127.0.0.1", cfg.mcpo_port, 1500);
    }
    if cfg.mcpo_port > 0 && platform::is_port_open("127.0.0.1", cfg.mcpo_port) {
        return Err(format!(
            "mcpo port {} is already in use by another process outside AI Control Center.",
            cfg.mcpo_port
        ));
    }

    let mut cmd = Command::new(&exe);
    cmd.current_dir(&cfg.owu_scripts_dir).args([
        "--port",
        &cfg.mcpo_port.to_string(),
        "--server-type",
        "streamable-http",
        "--",
        cfg.mcpo_url.trim(),
    ]);
    platform::prepare_managed_command(&mut cmd);

    let child = cmd
        .new_console()
        .spawn()
        .map_err(|e| format!("Failed starting mcpo: {e}"))?;
    let pid = child.id();
    let start_id = platform::get_process_start_id(pid);
    record_tracked(
        TrackedSlot::Mcpo,
        TrackedProc {
            pid,
            exe_name: mcpo_exe,
            start_id,
            label: "mcpo".to_string(),
            routing_set_id: String::new(),
            routing_mode: String::new(),
        },
    );
    Ok(())
}

pub fn start_companion_service(svc: &crate::config::CompanionService) -> Result<(), String> {
    if svc.start_cmd.trim().is_empty() {
        return Err(format!("No start script configured for '{}'.", svc.name));
    }
    platform::run_script_background(&svc.start_cmd)
}

pub fn stop_companion_service(svc: &crate::config::CompanionService) -> Result<(), String> {
    if svc.stop_cmd.trim().is_empty() {
        return Err(format!("No stop script configured for '{}'.", svc.name));
    }
    platform::run_script_background(&svc.stop_cmd)
}

pub fn run_bat_file(bat_path: &str) -> Result<(), String> {
    platform::run_script_in_terminal(bat_path)
}

pub fn copy_to_clipboard(text: &str) -> bool {
    platform::copy_to_clipboard(text)
}

pub fn read_from_clipboard() -> Option<String> {
    platform::read_from_clipboard()
}

pub fn open_text_in_notepad(filename: &str, text: &str) {
    platform::open_text_in_editor(filename, text);
}

pub fn open_existing_file_in_notepad(file_path: &str) {
    platform::open_existing_file_in_editor(file_path);
}
