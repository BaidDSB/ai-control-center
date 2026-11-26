use crate::config::{ModelProfile, RoutingSet, StackConfig};
use crate::platform::{self, CommandExtPlatform};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const MAX_CONSOLE_LINES: usize = 3500;
const RUNNING_STATE_FILE: &str = "running_state.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiNotifyTarget {
    Root,
    Console,
    Both,
}

pub type UiNotifier = Arc<dyn Fn(UiNotifyTarget) + Send + Sync>;

pub fn noop_notifier() -> UiNotifier {
    Arc::new(|_| {})
}

#[derive(Clone, Copy, Default)]
pub struct ProcStatus {
    pub llama: bool,
    pub owu: bool,
    pub mcpo: bool,
    pub companion_running: [bool; 8],
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedProc {
    pub pid: u32,
    pub exe_name: String,
    #[serde(default)]
    pub start_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub routing_set_id: String,
    #[serde(default)]
    pub routing_mode: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunningState {
    #[serde(default)]
    pub backend: Option<TrackedProc>,
    #[serde(default)]
    pub frontend: Option<TrackedProc>,
    #[serde(default)]
    pub mcpo: Option<TrackedProc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackedSlot {
    Backend,
    Frontend,
    Mcpo,
}

#[derive(Clone, Debug, Default)]
struct TrackedMeta {
    label: String,
    routing_set_id: String,
    routing_mode: String,
}

impl RunningState {
    pub fn parse_toml(raw: &str) -> Self {
        toml::from_str(raw).unwrap_or_default()
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    fn state_path() -> String {
        StackConfig::resolve_config_relative(RUNNING_STATE_FILE)
    }

    fn load_from_disk() -> Self {
        let path = Self::state_path();
        fs::read_to_string(path)
            .map(|raw| Self::parse_toml(&raw))
            .unwrap_or_default()
    }

    fn save_to_disk(&self) {
        let path = Self::state_path();
        if self.backend.is_none() && self.frontend.is_none() && self.mcpo.is_none() {
            let _ = fs::remove_file(path);
            return;
        }
        if let Ok(text) = self.to_toml() {
            let _ = fs::write(path, text);
        }
    }

    fn slot_mut(&mut self, slot: TrackedSlot) -> &mut Option<TrackedProc> {
        match slot {
            TrackedSlot::Backend => &mut self.backend,
            TrackedSlot::Frontend => &mut self.frontend,
            TrackedSlot::Mcpo => &mut self.mcpo,
        }
    }
}

static RUNNING_STATE: OnceLock<Mutex<RunningState>> = OnceLock::new();

fn state_lock() -> &'static Mutex<RunningState> {
    RUNNING_STATE.get_or_init(|| Mutex::new(RunningState::load_from_disk()))
}

pub fn init_running_state() -> RunningState {
    let mut loaded = RunningState::load_from_disk();
    let mut changed = false;

    for slot in [&mut loaded.backend, &mut loaded.frontend, &mut loaded.mcpo] {
        if let Some(tp) = slot.as_ref() {
            if !platform::verify_process_identity(tp.pid, &tp.exe_name, &tp.start_id) {
                *slot = None;
                changed = true;
            }
        }
    }

    if changed {
        loaded.save_to_disk();
    }
    if let Ok(mut guard) = state_lock().lock() {
        *guard = loaded.clone();
    }
    loaded
}

pub fn active_backend_info() -> Option<TrackedProc> {
    state_lock().lock().ok().and_then(|g| g.backend.clone())
}

fn record_tracked(slot: TrackedSlot, proc_info: TrackedProc) {
    if let Ok(mut guard) = state_lock().lock() {
        *guard.slot_mut(slot) = Some(proc_info);
        guard.save_to_disk();
    }
}

fn clear_tracked_if_pid(slot: TrackedSlot, pid: u32) {
    if let Ok(mut guard) = state_lock().lock() {
        let slot_ref = guard.slot_mut(slot);
        if slot_ref.as_ref().map(|t| t.pid) == Some(pid) {
            *slot_ref = None;
            guard.save_to_disk();
        }
    }
}

fn stop_tracked_slot(slot: TrackedSlot) -> bool {
    let entry = if let Ok(mut guard) = state_lock().lock() {
        let taken = guard.slot_mut(slot).take();
        if taken.is_some() {
            guard.save_to_disk();
        }
        taken
    } else {
        None
    };

    if let Some(tp) = entry {
        if platform::verify_process_identity(tp.pid, &tp.exe_name, &tp.start_id) {
            platform::stop_process_tree(tp.pid);
            return true;
        }
    }
    false
}

pub fn stop_tracked_backend() -> bool {
    stop_tracked_slot(TrackedSlot::Backend)
}

pub fn stop_tracked_frontend() -> bool {
    stop_tracked_slot(TrackedSlot::Frontend)
}

pub fn stop_tracked_mcpo() -> bool {
    stop_tracked_slot(TrackedSlot::Mcpo)
}

pub fn poll_tracked_processes(companion_ports: &[u16]) -> ProcStatus {
    let snapshot = state_lock()
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    let check_alive = |opt: &Option<TrackedProc>| -> bool {
        opt.as_ref()
            .map(|tp| platform::is_pid_running_as(tp.pid, &tp.exe_name))
            .unwrap_or(false)
    };

    let backend_on = check_alive(&snapshot.backend);
    let frontend_on = check_alive(&snapshot.frontend);
    let mcpo_on = check_alive(&snapshot.mcpo);

    let mut pruned = false;
    if let Ok(mut guard) = state_lock().lock() {
        if !backend_on {
            if let Some(snap) = snapshot.backend.as_ref() {
                if guard.backend.as_ref().map(|t| t.pid) == Some(snap.pid) {
                    guard.backend = None;
                    pruned = true;
                }
            }
        }
        if !frontend_on {
            if let Some(snap) = snapshot.frontend.as_ref() {
                if guard.frontend.as_ref().map(|t| t.pid) == Some(snap.pid) {
                    guard.frontend = None;
                    pruned = true;
                }
            }
        }
        if !mcpo_on {
            if let Some(snap) = snapshot.mcpo.as_ref() {
                if guard.mcpo.as_ref().map(|t| t.pid) == Some(snap.pid) {
                    guard.mcpo = None;
                    pruned = true;
                }
            }
        }
        if pruned {
            guard.save_to_disk();
        }
    }

    let mut comp = [false; 8];
    for (i, &port) in companion_ports.iter().take(8).enumerate() {
        comp[i] = platform::is_port_open("127.0.0.1", port);
    }

    ProcStatus {
        llama: backend_on,
        owu: frontend_on,
        mcpo: mcpo_on,
        companion_running: comp,
    }
}

fn normalize_probe_host(host: &str) -> &str {
    let h = host.trim();
    if h.is_empty() || h == "0.0.0.0" || h == "::" || h == "[::]" {
        "127.0.0.1"
    } else {
        h
    }
}

fn wait_for_port_release(host: &str, port: u16, max_wait_ms: u64) {
    if port == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_millis(max_wait_ms);
    while Instant::now() < deadline {
        if !platform::is_port_open(host, port) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

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
    for pid in &set.profile_ids {
        let Some(p) = cfg.find_profile(pid) else {
            return Err(format!("linked profile '{pid}' missing"));
        };
        let (cmd_check, _) =
            p.build_swap_cmd_and_env_with_engines(&cfg.models_dir, &cfg.llama_dir, &cfg.saved_engines);
        if !cmd_check.contains("${PORT}") {
            return Err(format!("profile '{}' is missing {{port}}/{{addr}}", p.name));
        }
    }
    let yaml = cfg.build_routing_set_yaml(set);
    fs::write(cfg.resolved_llama_swap_yaml_path(), yaml)
        .map_err(|e| format!("failed writing llama-swap.yaml: {e}"))?;
    Ok(true)
}

pub struct ConsoleBuffer {
    pub lines: VecDeque<String>,
    pub auto_scroll: bool,
    pub filter: String,
    pub total_lines: u64,
}

impl Default for ConsoleBuffer {
    fn default() -> Self {
        Self {
            lines: VecDeque::with_capacity(512),
            auto_scroll: true,
            filter: String::new(),
            total_lines: 0,
        }
    }
}

impl ConsoleBuffer {
    pub fn push(&mut self, line: String) {
        if self.lines.len() >= MAX_CONSOLE_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        self.total_lines = self.total_lines.saturating_add(1);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn full_text(&self) -> String {
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(l);
        }
        out
    }
}

pub type SharedConsole = Arc<Mutex<ConsoleBuffer>>;
pub type SharedErrors = Arc<Mutex<Vec<String>>>;

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

pub fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for nc in chars.by_ref() {
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

fn is_critical_error_line(line_lower: &str) -> bool {
    line_lower.contains("cuda error")
        || line_lower.contains("out of memory")
        || line_lower.contains("failed to load model")
        || line_lower.contains("unable to load model")
        || line_lower.contains("error: invalid argument")
        || line_lower.contains("error: unknown argument")
        || line_lower.contains("unknown option")
        || line_lower.contains("traceback (most recent call last)")
        || line_lower.contains("address already in use")
        || line_lower.contains("fatal:")
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    reader: R,
    prefix: &'static str,
    tag: &'static str,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
) {
    thread::spawn(move || {
        let mut buf_reader = BufReader::new(reader);
        let mut raw = Vec::with_capacity(256);
        let mut last_err_logged: Option<String> = None;

        loop {
            raw.clear();
            match buf_reader.read_until(b'\n', &mut raw) {
                Ok(0) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&raw);
                    let cleaned = strip_ansi_codes(text.trim_end_matches(['\r', '\n']));
                    if cleaned.is_empty() {
                        continue;
                    }
                    let formatted = if prefix.is_empty() {
                        cleaned.clone()
                    } else {
                        format!("{prefix}{cleaned}")
                    };
                    if let Ok(mut c) = console.lock() {
                        c.push(formatted);
                    }
                    let lower = cleaned.to_ascii_lowercase();
                    if is_critical_error_line(&lower)
                        && last_err_logged.as_deref() != Some(cleaned.as_str())
                    {
                        last_err_logged = Some(cleaned.clone());
                        if let Ok(mut errs) = errors.lock() {
                            let num = errs.len() + 1;
                            errs.push(format!("[{tag} ERR #{num}] {cleaned}"));
                        }
                        notify(UiNotifyTarget::Both);
                    } else {
                        notify(UiNotifyTarget::Console);
                    }
                }
                Err(_) => break,
            }
        }
    });
}

pub fn poll_processes_once(_backend_exes: &[String]) -> ProcStatus {
    poll_tracked_processes(&[])
}

pub fn poll_processes_full(
    _backend_exes: &[String],
    _frontend_exe: &str,
    companion_ports: &[u16],
) -> ProcStatus {
    poll_tracked_processes(companion_ports)
}

pub fn stop_process(exe_name: &str) {
    platform::stop_process_by_name(exe_name);
}

pub fn stop_all_backends(_backend_exes: &[String]) {
    stop_tracked_backend();
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

    let args = profile.build_llama_args(&cfg.models_dir);
    let disable_pinned = profile.should_disable_cuda_pinned();
    let mut headers = vec![format!("[ENGINE] {engine_kind} -> {}", exe.display())];
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
        let ini = cfg.build_routing_set_ini(set);
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
        let headers = vec![
            format!(
                "[ROUTING SET (models.ini)] {} ({} models)",
                set.name,
                linked.len()
            ),
            format!("[LAUNCH] {engine_exe} {}", args.join(" ")),
        ];
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
    for p in &linked {
        let (cmd_check, _) =
            p.build_swap_cmd_and_env_with_engines(&cfg.models_dir, &cfg.llama_dir, &cfg.saved_engines);
        if !cmd_check.contains("${PORT}") {
            return Err(format!(
                "Profile '{}' uses a custom engine with no launch flags. Set Custom Flags on the profile (or Default Args on the engine) including {{model}} and {{port}}/{{addr}} (e.g. serve --model {{model}} --ctx {{ctx}} --addr {{addr}}).",
                p.name
            ));
        }
    }
    let yaml = cfg.build_routing_set_yaml(set);
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
    let headers = vec![
        format!(
            "[ROUTING SET (llama-swap)] {} ({} models on http://{}/v1)",
            set.name,
            linked.len(),
            listen
        ),
        format!("[LAUNCH] {swap_name} {}", args.join(" ")),
    ];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_state_toml_roundtrip_and_invalid_fallback() {
        let state = RunningState {
            backend: Some(TrackedProc {
                pid: 4512,
                exe_name: "llama-swap.exe".to_string(),
                start_id: "638990012345".to_string(),
                label: "Routing Set: Daily [llama_swap]".to_string(),
                routing_set_id: "daily-1".to_string(),
                routing_mode: "llama_swap".to_string(),
            }),
            frontend: Some(TrackedProc {
                pid: 8810,
                exe_name: "open-webui.exe".to_string(),
                start_id: "638990099999".to_string(),
                label: "Open WebUI".to_string(),
                routing_set_id: String::new(),
                routing_mode: String::new(),
            }),
            mcpo: None,
        };

        let serialized = state.to_toml().expect("RunningState should serialize");
        let parsed = RunningState::parse_toml(&serialized);
        assert_eq!(parsed, state);

        let corrupted = RunningState::parse_toml("not valid = [ toml");
        assert_eq!(corrupted, RunningState::default());
    }

    #[test]
    fn strip_ansi_removes_escape_codes_and_cr() {
        let raw = "\x1b[1;32m[INFO]\x1b[0m Loaded model\r";
        assert_eq!(strip_ansi_codes(raw), "[INFO] Loaded model");
    }

    #[test]
    fn critical_error_detector_matches_real_failures() {
        assert!(is_critical_error_line("cuda error: out of memory"));
        assert!(is_critical_error_line("error: unknown argument --bad-flag"));
        assert!(is_critical_error_line("bind failed: address already in use"));
        assert!(!is_critical_error_line("http server listening on 127.0.0.1:8080"));
    }

    #[test]
    fn probe_host_normalizes_wildcards() {
        assert_eq!(normalize_probe_host("0.0.0.0"), "127.0.0.1");
        assert_eq!(normalize_probe_host("::"), "127.0.0.1");
        assert_eq!(normalize_probe_host(""), "127.0.0.1");
        assert_eq!(normalize_probe_host(" 192.168.1.50 "), "192.168.1.50");
    }

    #[test]
    fn console_buffer_enforces_max_line_cap() {
        let mut buf = ConsoleBuffer::default();
        for i in 0..(MAX_CONSOLE_LINES + 5) {
            buf.push(format!("line-{i}"));
        }
        assert_eq!(buf.lines.len(), MAX_CONSOLE_LINES);
        assert_eq!(buf.total_lines, (MAX_CONSOLE_LINES + 5) as u64);
        assert_eq!(buf.lines.front().map(String::as_str), Some("line-5"));
    }
}