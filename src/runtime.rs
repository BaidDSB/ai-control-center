use crate::config::{ModelProfile, RoutingSet, StackConfig};
use eframe::egui;
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use crate::platform::{self, CommandExtPlatform};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

const MAX_CONSOLE_LINES: usize = 3500;

#[derive(Clone, Copy, Default)]
pub struct ProcStatus {
    pub llama: bool,
    pub owu: bool,
    pub mcpo: bool,
    pub companion_running: [bool; 8],
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
            if i > 0 { out.push('\n'); }
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
        if trimmed.is_empty() { return; }
        if !exes.iter().any(|e| e.eq_ignore_ascii_case(trimmed)) {
            exes.push(trimmed.to_string());
        }
    };
    if let Some(fname) = Path::new(&cfg.llama_swap_exe).file_name().and_then(|s| s.to_str()) {
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
                    if ('@'..='~').contains(&nc) { break; }
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
    ctx: egui::Context,
    vp_id: egui::ViewportId,
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
                    if cleaned.is_empty() { continue; }
                    let formatted = if prefix.is_empty() { cleaned.clone() } else { format!("{prefix}{cleaned}") };
                    if let Ok(mut c) = console.lock() {
                        c.push(formatted);
                    }
                    let lower = cleaned.to_ascii_lowercase();
                    if is_critical_error_line(&lower) && last_err_logged.as_deref() != Some(cleaned.as_str()) {
                        last_err_logged = Some(cleaned.clone());
                        if let Ok(mut errs) = errors.lock() {
                            let num = errs.len() + 1;
                            errs.push(format!("[{tag} ERR #{num}] {cleaned}"));
                        }
                        ctx.request_repaint_of(egui::ViewportId::ROOT);
                    }
                    ctx.request_repaint_of(vp_id);
                }
                Err(_) => break,
            }
        }
    });
}

pub fn poll_processes_once(backend_exes: &[String]) -> ProcStatus {
    poll_processes_full(backend_exes, &platform::default_exe_name("open-webui"), &[])
}

pub fn poll_processes_full(
    backend_exes: &[String],
    frontend_exe: &str,
    companion_ports: &[u16],
) -> ProcStatus {
    let mut targets: Vec<String> = backend_exes.to_vec();
    if !frontend_exe.trim().is_empty() {
        targets.push(frontend_exe.trim().to_string());
    }
    let mcpo_exe = platform::default_exe_name("mcpo");
    targets.push(mcpo_exe.clone());

    let running = platform::poll_running_exes(&targets);
    let any_backend = backend_exes.iter().any(|e| running.contains(&e.trim().to_ascii_lowercase()));
    let fe_on = !frontend_exe.trim().is_empty()
        && running.contains(&frontend_exe.trim().to_ascii_lowercase());
    let mcpo_on = running.contains(&mcpo_exe.to_ascii_lowercase());

    let mut comp = [false; 8];
    for (i, &port) in companion_ports.iter().take(8).enumerate() {
        comp[i] = platform::is_port_open("127.0.0.1", port);
    }

    ProcStatus {
        llama: any_backend,
        owu: fe_on,
        mcpo: mcpo_on,
        companion_running: comp,
    }
}

pub fn stop_process(exe_name: &str) {
    platform::stop_process_by_name(exe_name);
}

pub fn stop_all_backends(backend_exes: &[String]) {
    let running = platform::poll_running_exes(backend_exes);
    for exe in backend_exes {
        if running.contains(&exe.trim().to_ascii_lowercase()) {
            platform::stop_process_by_name(exe);
        }
    }
}

fn spawn_monitored(
    exe: &Path,
    work_dir: &Path,
    args: &[String],
    disable_pinned: bool,
    use_external_cmd: bool,
    header_lines: &[String],
    tag: &'static str,
    console: SharedConsole,
    errors: SharedErrors,
    ctx: egui::Context,
    vp_id: egui::ViewportId,
) -> Result<(), String> {
    let exe_name = exe.file_name().and_then(|s| s.to_str()).unwrap_or("engine.exe").to_string();
    let mut cmd = Command::new(exe);
    cmd.current_dir(work_dir).args(args);
    if disable_pinned {
        cmd.env("GGML_CUDA_NO_PINNED", "1");
    }

    if use_external_cmd {
        cmd.new_console()
            .spawn()
            .map_err(|e| format!("Failed starting {exe_name}: {e}"))?;
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

    if let Some(stdout) = child.stdout.take() {
        spawn_pipe_reader(stdout, "", tag, Arc::clone(&console), Arc::clone(&errors), ctx.clone(), vp_id);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_pipe_reader(stderr, "", tag, Arc::clone(&console), Arc::clone(&errors), ctx.clone(), vp_id);
    }

    let wait_console = Arc::clone(&console);
    let wait_ctx = ctx.clone();
    thread::spawn(move || {
        let status = child.wait();
        if let Ok(mut c) = wait_console.lock() {
            match status {
                Ok(s) => c.push(format!("[{exe_name} EXITED] {s}")),
                Err(e) => c.push(format!("[{exe_name} WAIT ERROR] {e}")),
            }
        }
        wait_ctx.request_repaint_of(vp_id);
        wait_ctx.request_repaint_of(egui::ViewportId::ROOT);
    });

    ctx.request_repaint_of(vp_id);
    Ok(())
}

pub fn launch_single_profile(
    cfg: &StackConfig,
    profile: &ModelProfile,
    console: SharedConsole,
    errors: SharedErrors,
    ctx: egui::Context,
    vp_id: egui::ViewportId,
) -> Result<(), String> {
    let engine_kind = profile.effective_engine_kind();
    let engine_dir = profile.effective_engine_dir(&cfg.llama_dir);
    let engine_exe = profile.effective_engine_exe();
    let exe = Path::new(engine_dir).join(engine_exe);

    if !exe.exists() {
        return Err(format!("Engine executable '{engine_exe}' not found at {}", exe.display()));
    }
    let model_path = Path::new(&cfg.models_dir).join(&profile.single.model_rel_path);
    if !model_path.exists() {
        return Err(format!("Model file not found: {}", model_path.display()));
    }

    let all_exes = collect_backend_exes(cfg);
    stop_all_backends(&all_exes);

    let args = profile.build_llama_args(&cfg.models_dir);
    let disable_pinned = profile.should_disable_cuda_pinned();
    let mut headers = vec![format!("[ENGINE] {engine_kind} -> {}", exe.display())];
    if disable_pinned {
        headers.push("[ENV] GGML_CUDA_NO_PINNED=1".to_string());
    }
    headers.push(format!("[LAUNCH] {engine_exe} {}", args.join(" ")));

    spawn_monitored(
        &exe,
        Path::new(engine_dir),
        &args,
        disable_pinned,
        cfg.use_external_cmd_windows,
        &headers,
        "ENGINE",
        console,
        errors,
        ctx,
        vp_id,
    )
}

pub fn launch_routing_set(
    cfg: &StackConfig,
    set: &RoutingSet,
    console: SharedConsole,
    errors: SharedErrors,
    ctx: egui::Context,
    vp_id: egui::ViewportId,
) -> Result<(), String> {
    if set.profile_ids.is_empty() {
        return Err(format!("Routing Set '{}' has no linked model profiles.", set.name));
    }

    let mut linked: Vec<&ModelProfile> = Vec::new();
    for pid in &set.profile_ids {
        let Some(p) = cfg.find_profile(pid) else {
            return Err(format!("Linked profile '{pid}' no longer exists."));
        };
        let model_path = Path::new(&cfg.models_dir).join(&p.single.model_rel_path);
        if !model_path.exists() {
            return Err(format!("Model file for '{}' not found: {}", p.name, model_path.display()));
        }
        linked.push(p);
    }

    let all_exes = collect_backend_exes(cfg);
    stop_all_backends(&all_exes);

    if set.mode == "llama_ini" {
        if linked.iter().any(|p| p.effective_engine_kind() == "custom") {
            return Err("Native models.ini mode only supports llama.cpp profiles. Switch set mode to 'llama_swap' for custom / mixed engines.".to_string());
        }
        let first = linked[0];
        let engine_dir = first.effective_engine_dir(&cfg.llama_dir);
        let engine_exe = first.effective_engine_exe();
        let exe = Path::new(engine_dir).join(engine_exe);
        if !exe.exists() {
            return Err(format!("Engine executable '{engine_exe}' not found at {}", exe.display()));
        }
        let ini = cfg.build_routing_set_ini(set);
        fs::write(&cfg.resolved_router_ini_path(), ini).map_err(|e| format!("Failed writing models.ini: {e}"))?;

        let mut args = vec![
            "--models-preset".to_string(), cfg.resolved_router_ini_path(),
            "--models-max".to_string(), "1".to_string(),
        ];
        if set.no_models_autoload {
            args.push("--no-models-autoload".to_string());
        }
        args.extend([
            "--host".to_string(), cfg.server_host.trim().to_string(),
            "--port".to_string(), cfg.server_port.to_string(),
        ]);
        let disable_pinned = linked.iter().any(|p| p.should_disable_cuda_pinned());
        let headers = vec![
            format!("[ROUTING SET (models.ini)] {} ({} models)", set.name, linked.len()),
            format!("[LAUNCH] {engine_exe} {}", args.join(" ")),
        ];
        return spawn_monitored(
            &exe,
            Path::new(engine_dir),
            &args,
            disable_pinned,
            cfg.use_external_cmd_windows,
            &headers,
            "ENGINE",
            console,
            errors,
            ctx,
            vp_id,
        );
    }

    // Default: Multi-Engine Routing via llama-swap
    let swap_exe = Path::new(&cfg.llama_swap_exe);
    if !swap_exe.exists() {
        return Err(format!("llama-swap.exe not found at {}", swap_exe.display()));
    }
    for p in &linked {
        let (cmd_check, _) = p.build_swap_cmd_and_env_with_engines(&cfg.models_dir, &cfg.llama_dir, &cfg.saved_engines);
        if !cmd_check.contains("${PORT}") {
            return Err(format!(
                "Profile '{}' uses a custom engine with no launch flags. Set Custom Flags on the profile (or Default Args on the engine) including {{model}} and {{port}}/{{addr}} (e.g. serve --model {{model}} --ctx {{ctx}} --addr {{addr}}).",
                p.name
            ));
        }
    }
    let yaml = cfg.build_routing_set_yaml(set);
    fs::write(&cfg.resolved_llama_swap_yaml_path(), yaml).map_err(|e| format!("Failed writing llama-swap.yaml: {e}"))?;

    let listen = format!("{}:{}", cfg.server_host.trim(), cfg.server_port);
    let args = vec![
        "-config".to_string(), cfg.resolved_llama_swap_yaml_path(),
        "-listen".to_string(), listen.clone(),
        "-watch-config".to_string(),
    ];
    let work_dir = swap_exe.parent().unwrap_or_else(|| Path::new("."));
    let headers = vec![
        format!("[ROUTING SET (llama-swap)] {} ({} models on http://{}/v1)", set.name, linked.len(), listen),
        format!("[LAUNCH] llama-swap.exe {}", args.join(" ")),
    ];
    spawn_monitored(
        swap_exe,
        work_dir,
        &args,
        false,
        cfg.use_external_cmd_windows,
        &headers,
        "SWAP",
        console,
        errors,
        ctx,
        vp_id,
    )
}

pub fn launch_open_webui(
    cfg: &StackConfig,
    console: SharedConsole,
    errors: SharedErrors,
    ctx: egui::Context,
    vp_id: egui::ViewportId,
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
    stop_process(&exe_name);

    if cfg.use_external_cmd_windows {
        Command::new(&exe)
            .current_dir(&work_dir)
            .args(&args)
            .new_console()
            .spawn()
            .map_err(|e| format!("Failed starting {fe_name}: {e}"))?;
        return Ok(());
    }

    if let Ok(mut c) = console.lock() {
        c.push("================================================================".to_string());
        c.push(format!("[LAUNCH] {exe_name} {}", args.join(" ")));
        c.push("================================================================".to_string());
    }

    let mut child = Command::new(&exe)
        .current_dir(&work_dir)
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .no_window()
        .spawn()
        .map_err(|e| format!("Failed starting {fe_name}: {e}"))?;

    if let Some(stdout) = child.stdout.take() {
        spawn_pipe_reader(stdout, "", "FE", Arc::clone(&console), Arc::clone(&errors), ctx.clone(), vp_id);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_pipe_reader(stderr, "", "FE", Arc::clone(&console), Arc::clone(&errors), ctx.clone(), vp_id);
    }

    let wait_console = Arc::clone(&console);
    let wait_ctx = ctx.clone();
    let tag_name = fe_name.clone();
    thread::spawn(move || {
        let status = child.wait();
        if let Ok(mut c) = wait_console.lock() {
            match status {
                Ok(s) => c.push(format!("[{tag_name} EXITED] {s}")),
                Err(e) => c.push(format!("[{tag_name} WAIT ERROR] {e}")),
            }
        }
        wait_ctx.request_repaint_of(vp_id);
        wait_ctx.request_repaint_of(egui::ViewportId::ROOT);
    });

    ctx.request_repaint_of(vp_id);
    Ok(())
}

pub fn launch_mcpo(cfg: &StackConfig) -> Result<(), String> {
    let mcpo_exe = platform::default_exe_name("mcpo");
    let exe = Path::new(&cfg.owu_scripts_dir).join(&mcpo_exe);
    if !exe.exists() {
        return Err(format!("{mcpo_exe} not found at {}", exe.display()));
    }
    stop_process(&mcpo_exe);
    Command::new(&exe)
        .current_dir(&cfg.owu_scripts_dir)
        .args([
            "--port", &cfg.mcpo_port.to_string(),
            "--server-type", "streamable-http",
            "--", cfg.mcpo_url.trim(),
        ])
        .new_console()
        .spawn()
        .map_err(|e| format!("Failed starting mcpo: {e}"))?;
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
