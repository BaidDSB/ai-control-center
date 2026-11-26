pub mod console;
pub mod launcher;
pub mod state;

pub use console::*;
pub use launcher::*;
pub use state::*;

#[cfg(test)]
mod tests {
    use super::*;
    use super::console::is_critical_error_line;
    use super::console::MAX_CONSOLE_LINES;
    use super::state::normalize_probe_host;

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
