use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
pub const CREATE_NEW_CONSOLE: u32 = 0x00000010;

pub fn apply_no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn apply_new_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

pub fn default_exe_name(base: &str) -> String {
    #[cfg(windows)]
    {
        if base.to_ascii_lowercase().ends_with(".exe") {
            base.to_string()
        } else {
            format!("{base}.exe")
        }
    }
    #[cfg(not(windows))]
    {
        base.strip_suffix(".exe").unwrap_or(base).to_string()
    }
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Portable-first: uses the executable's directory whenever writable (keeps Windows portable installs unchanged).
/// Falls back to ~/.config/ai-control-center on read-only installs (e.g., Linux /usr/local/bin).
pub fn resolve_config_dir(config_filename: &str) -> PathBuf {
    let dir = exe_dir();
    let probe = dir.join(".cc_write_probe.tmp");
    if dir.join(config_filename).exists() {
        if fs::OpenOptions::new().append(true).open(dir.join(config_filename)).is_ok() {
            return dir;
        }
    } else if fs::write(&probe, b"").is_ok() {
        let _ = fs::remove_file(&probe);
        return dir;
    }

    #[cfg(unix)]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| dir.clone());
        let cfg_dir = base.join("ai-control-center");
        let _ = fs::create_dir_all(&cfg_dir);
        return cfg_dir;
    }

    #[cfg(windows)]
    {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            let cfg_dir = PathBuf::from(app_data).join("ai-control-center");
            let _ = fs::create_dir_all(&cfg_dir);
            return cfg_dir;
        }
        dir
    }
}

pub fn is_port_open(host: &str, port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let target = format!("{}:{port}", if host.trim().is_empty() { "127.0.0.1" } else { host.trim() });
    let addrs: Vec<SocketAddr> = target
        .to_socket_addrs()
        .map(|it| it.collect())
        .unwrap_or_default();
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(120)).is_ok() {
            return true;
        }
    }
    false
}

pub fn poll_running_exes(targets: &[String]) -> HashSet<String> {
    let mut running = HashSet::new();
    if targets.is_empty() {
        return running;
    }

    #[cfg(windows)]
    {
        let mut cmd = Command::new("tasklist");
        cmd.args(["/FO", "CSV", "/NH"]);
        apply_no_window(&mut cmd);
        if let Ok(out) = cmd.output() {
            let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            for t in targets {
                let low = t.trim().to_ascii_lowercase();
                if !low.is_empty() && text.contains(&format!("\"{low}\"")) {
                    running.insert(low);
                }
            }
        }
    }

    #[cfg(unix)]
    {
        let mut hay = String::new();
        if let Ok(entries) = fs::read_dir("/proc") {
            for e in entries.flatten() {
                if !e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                if let Ok(comm) = fs::read_to_string(e.path().join("comm")) {
                    hay.push_str(&comm.to_ascii_lowercase());
                    hay.push('\n');
                }
                if let Ok(cmdline) = fs::read(e.path().join("cmdline")) {
                    let s = String::from_utf8_lossy(&cmdline).replace('\0', " ").to_ascii_lowercase();
                    hay.push_str(&s);
                    hay.push('\n');
                }
            }
        }
        for t in targets {
            let low = t.trim().to_ascii_lowercase();
            let stripped = low.strip_suffix(".exe").unwrap_or(&low);
            if !stripped.is_empty() && hay.contains(stripped) {
                running.insert(low);
            }
        }
    }

    running
}

pub fn stop_process_by_name(exe_name: &str) {
    let name = exe_name.trim();
    if name.is_empty() {
        return;
    }

    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/F", "/T", "/IM", name]);
        apply_no_window(&mut cmd);
        let _ = cmd.output();
    }

    #[cfg(unix)]
    {
        let clean = name.strip_suffix(".exe").unwrap_or(name);
        let _ = Command::new("pkill").args(["-TERM", "-f", clean]).output();
        std::thread::sleep(Duration::from_millis(250));
        let _ = Command::new("pkill").args(["-KILL", "-f", clean]).output();
    }
}

pub fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "clip"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_no_window(&mut cmd);
        if let Ok(mut child) = cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            return child.wait().map(|s| s.success()).unwrap_or(false);
        }
        false
    }

    #[cfg(unix)]
    {
        let tools: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        for (bin, args) in tools {
            if let Ok(mut child) = Command::new(bin)
                .args(*args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                if child.wait().map(|s| s.success()).unwrap_or(false) {
                    return true;
                }
            }
        }
        false
    }
}

pub fn read_from_clipboard() -> Option<String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", "Get-Clipboard -Raw"]);
        apply_no_window(&mut cmd);
        let out = cmd.output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        if s.is_empty() { None } else { Some(s) }
    }

    #[cfg(unix)]
    {
        let tools: &[(&str, &[&str])] = &[
            ("wl-paste", &["-n"]),
            ("xclip", &["-selection", "clipboard", "-o"]),
            ("xsel", &["--clipboard", "--output"]),
        ];
        for (bin, args) in tools {
            if let Ok(out) = Command::new(bin).args(*args).output() {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
        None
    }
}

pub fn open_url(url: &str) {
    let u = url.trim();
    if u.is_empty() {
        return;
    }

    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", u]);
        apply_no_window(&mut cmd);
        let _ = cmd.spawn();
    }

    #[cfg(unix)]
    {
        let _ = Command::new("xdg-open").arg(u).spawn();
    }
}

pub fn open_existing_file_in_editor(path: &str) {
    let p = path.trim();
    if p.is_empty() {
        return;
    }

    #[cfg(windows)]
    {
        let mut cmd = Command::new("notepad.exe");
        cmd.arg(p);
        let _ = cmd.spawn();
    }

    #[cfg(unix)]
    {
        let _ = Command::new("xdg-open").arg(p).spawn();
    }
}

pub fn open_text_in_editor(filename: &str, text: &str) {
    let tmp = std::env::temp_dir().join(filename);
    if fs::write(&tmp, text).is_ok() {
        open_existing_file_in_editor(&tmp.to_string_lossy());
    }
}

pub fn run_script_in_terminal(script_path: &str) -> Result<(), String> {
    let p = Path::new(script_path.trim());
    if !p.exists() {
        return Err(format!("Script not found: {}", p.display()));
    }
    let parent = p.parent().unwrap_or_else(|| Path::new("."));

    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.current_dir(parent).args(["/C", "start", "", &p.to_string_lossy()]);
        apply_no_window(&mut cmd);
        cmd.spawn().map_err(|e| format!("Failed to launch script: {e}"))?;
        Ok(())
    }

    #[cfg(unix)]
    {
        let s = p.to_string_lossy().to_string();
        let terms: &[(&str, Vec<String>)] = &[
            ("x-terminal-emulator", vec!["-e".into(), "sh".into(), s.clone()]),
            ("gnome-terminal", vec!["--".into(), "sh".into(), s.clone()]),
            ("konsole", vec!["-e".into(), "sh".into(), s.clone()]),
            ("xfce4-terminal", vec!["-e".into(), format!("sh \"{s}\"")]),
        ];
        for (bin, args) in terms {
            if Command::new(bin).current_dir(parent).args(args).spawn().is_ok() {
                return Ok(());
            }
        }
        Command::new("sh")
            .current_dir(parent)
            .arg(&s)
            .spawn()
            .map_err(|e| format!("Failed to launch script: {e}"))?;
        Ok(())
    }
}

pub fn run_script_background(script_path: &str) -> Result<(), String> {
    let p = Path::new(script_path.trim());
    if !p.exists() {
        return Err(format!("Script not found: {}", p.display()));
    }
    let parent = p.parent().unwrap_or_else(|| Path::new("."));

    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.current_dir(parent)
            .env("CI", "1")
            .args(["/C", &p.to_string_lossy()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_no_window(&mut cmd);
        cmd.spawn().map_err(|e| format!("Failed to run {}: {e}", p.display()))?;
        Ok(())
    }

    #[cfg(unix)]
    {
        Command::new("sh")
            .current_dir(parent)
            .arg(p)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to run {}: {e}", p.display()))?;
        Ok(())
    }
}

pub fn spawn_in_external_terminal(
    title: &str,
    exe: &Path,
    args: &[String],
    work_dir: &Path,
    envs: &[(String, String)],
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.current_dir(work_dir);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut full = vec!["/C".to_string(), "start".to_string(), title.to_string(), exe.to_string_lossy().to_string()];
        full.extend(args.iter().cloned());
        cmd.args(&full).creation_flags(CREATE_NO_WINDOW);
        cmd.spawn().map_err(|e| format!("External terminal launch failed: {e}"))?;
        Ok(())
    }

    #[cfg(unix)]
    {
        let _ = title;
        let mut cmd = Command::new(exe);
        cmd.current_dir(work_dir).args(args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.spawn().map_err(|e| format!("Process launch failed: {e}"))?;
        Ok(())
    }
}

pub trait CommandExtPlatform {
    fn no_window(&mut self) -> &mut Self;
    fn new_console(&mut self) -> &mut Self;
}

impl CommandExtPlatform for Command {
    fn no_window(&mut self) -> &mut Self {
        apply_no_window(self);
        self
    }

    fn new_console(&mut self) -> &mut Self {
        apply_new_console(self);
        self
    }
}

// =========================================================================
// PID-Based Process Verification & Process-Tree Termination (v1.1)
// =========================================================================

pub fn prepare_managed_command(cmd: &mut std::process::Command) {
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        let _ = cmd;
    }
}

pub fn exe_name_matches(actual: &str, expected: &str) -> bool {
    let a = actual.trim().to_ascii_lowercase();
    let e = expected.trim().to_ascii_lowercase();
    if a.is_empty() || e.is_empty() {
        return false;
    }
    if a == e {
        return true;
    }
    let a_stem = a.strip_suffix(".exe").unwrap_or(&a);
    let e_stem = e.strip_suffix(".exe").unwrap_or(&e);
    #[cfg(windows)]
    {
        a_stem == e_stem
    }
    #[cfg(not(windows))]
    {
        a_stem == e_stem || (a.len() == 15 && e.starts_with(&a))
    }
}

#[cfg(windows)]
pub fn is_pid_running_as(pid: u32, expected_exe: &str) -> bool {
    if pid == 0 {
        return false;
    }
    let filter = format!("PID eq {pid}");
    let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .no_window()
        .output()
    else {
        return false;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }
        let cols: Vec<&str> = trimmed
            .trim_start_matches('"')
            .trim_end_matches('"')
            .split("\",\"")
            .collect();
        if cols.len() >= 2 && cols[1].trim().parse::<u32>().ok() == Some(pid) {
            return exe_name_matches(cols[0], expected_exe);
        }
    }
    false
}

#[cfg(not(windows))]
pub fn is_pid_running_as(pid: u32, expected_exe: &str) -> bool {
    if pid == 0 {
        return false;
    }
    let proc_dir = format!("/proc/{pid}");
    if !std::path::Path::new(&proc_dir).exists() {
        return false;
    }
    if let Ok(stat) = std::fs::read_to_string(format!("{proc_dir}/stat")) {
        if let Some(after) = stat.rfind(')').map(|i| &stat[i + 1..]) {
            if after.split_whitespace().next() == Some("Z") {
                return false;
            }
        }
    }
    if let Ok(comm) = std::fs::read_to_string(format!("{proc_dir}/comm")) {
        if exe_name_matches(&comm, expected_exe) {
            return true;
        }
    }
    if let Ok(raw) = std::fs::read(format!("{proc_dir}/cmdline")) {
        for arg in raw.split(|&b| b == 0).take(2) {
            if arg.is_empty() {
                continue;
            }
            let s = String::from_utf8_lossy(arg);
            if let Some(fname) = std::path::Path::new(s.as_ref())
                .file_name()
                .and_then(|n| n.to_str())
            {
                if exe_name_matches(fname, expected_exe) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(windows)]
pub fn get_process_start_id(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    let script = format!(
        "try {{ (Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToUniversalTime().Ticks }} catch {{}}"
    );
    let Ok(out) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .no_window()
        .output()
    else {
        return String::new();
    };
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[cfg(not(windows))]
pub fn get_process_start_id(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return String::new();
    };
    let Some(after) = stat.rfind(')').map(|i| &stat[i + 1..]) else {
        return String::new();
    };
    after
        .split_whitespace()
        .nth(19)
        .unwrap_or_default()
        .to_string()
}

pub fn verify_process_identity(pid: u32, expected_exe: &str, expected_start_id: &str) -> bool {
    if !is_pid_running_as(pid, expected_exe) {
        return false;
    }
    if !expected_start_id.trim().is_empty() {
        let current = get_process_start_id(pid);
        if current != expected_start_id.trim() {
            return false;
        }
    }
    true
}

#[cfg(windows)]
pub fn stop_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .no_window()
        .output();
}

#[cfg(not(windows))]
pub fn stop_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    let term_cmd = format!("kill -TERM -{pid} 2>/dev/null || kill -TERM {pid} 2>/dev/null");
    let _ = std::process::Command::new("sh")
        .args(["-c", &term_cmd])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(300));
    if std::path::Path::new(&format!("/proc/{pid}")).exists() {
        let kill_cmd = format!("kill -KILL -{pid} 2>/dev/null || kill -KILL {pid} 2>/dev/null");
        let _ = std::process::Command::new("sh")
            .args(["-c", &kill_cmd])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_name_matching_handles_case_and_extension() {
        assert!(exe_name_matches("llama-server.exe", "llama-server.exe"));
        assert!(exe_name_matches("LLAMA-SERVER.EXE", "llama-server"));
        assert!(exe_name_matches("llama-swap", "llama-swap.exe"));
        assert!(!exe_name_matches("llama-server.exe", "llama-swap.exe"));
        assert!(!exe_name_matches("", "llama-server"));
    }

    #[test]
    fn default_exe_name_matches_target_os() {
        let name = default_exe_name("llama-server");
        if cfg!(windows) {
            assert_eq!(name, "llama-server.exe");
        } else {
            assert_eq!(name, "llama-server");
        }
    }

    #[test]
    fn zero_pid_never_verifies() {
        assert!(!verify_process_identity(0, "llama-server", ""));
    }
}