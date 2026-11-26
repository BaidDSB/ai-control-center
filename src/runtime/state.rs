use crate::config::StackConfig;
use crate::platform;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const RUNNING_STATE_FILE: &str = "running_state.toml";

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
pub(crate) enum TrackedSlot {
    Backend,
    Frontend,
    Mcpo,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TrackedMeta {
    pub(crate) label: String,
    pub(crate) routing_set_id: String,
    pub(crate) routing_mode: String,
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

pub(crate) fn record_tracked(slot: TrackedSlot, proc_info: TrackedProc) {
    if let Ok(mut guard) = state_lock().lock() {
        *guard.slot_mut(slot) = Some(proc_info);
        guard.save_to_disk();
    }
}

pub(crate) fn clear_tracked_if_pid(slot: TrackedSlot, pid: u32) {
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

pub(crate) fn normalize_probe_host(host: &str) -> &str {
    let h = host.trim();
    if h.is_empty() || h == "0.0.0.0" || h == "::" || h == "[::]" {
        "127.0.0.1"
    } else {
        h
    }
}

pub(crate) fn wait_for_port_release(host: &str, port: u16, max_wait_ms: u64) {
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
