pub mod console;
pub mod launcher;
pub mod state;
pub mod telemetry;

pub use console::*;
pub use launcher::*;
pub use state::*;
pub use telemetry::*;

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

    #[test]
    fn probe_tcp_endpoint_detects_live_and_closed_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let addr = listener.local_addr().expect("local addr").to_string();
        assert!(probe_tcp_endpoint(&addr, 120));
        drop(listener);
        assert!(!probe_tcp_endpoint(&addr, 120));
    }

    #[test]
    fn parse_llama_swap_running_json_extracts_models_and_handles_empty() {
        let json = r#"{"running":[{"model":"qwen3.6-35b-a3b","state":"ready"},{"id":"bge-m3","state":"loading"}]}"#;
        let parsed = parse_llama_swap_running_json(json).expect("valid running json");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].model, "qwen3.6-35b-a3b");
        assert_eq!(parsed[0].state, "ready");
        assert_eq!(parsed[1].model, "bge-m3");
        assert_eq!(parsed[1].state, "loading");

        let empty = parse_llama_swap_running_json(r#"{"running":[]}"#).expect("empty running json");
        assert!(empty.is_empty());
        assert!(parse_llama_swap_running_json("not json").is_none());
    }

    #[test]
    fn parse_nvidia_smi_csv_reads_used_total_and_name() {
        let sample = "2145, 8192, NVIDIA GeForce RTX 3070\n";
        let info = parse_nvidia_smi_csv(sample).expect("parse nvidia-smi csv");
        assert_eq!(info.used_mib, 2145);
        assert_eq!(info.total_mib, 8192);
        assert_eq!(info.name, "NVIDIA GeForce RTX 3070");
        assert!((info.total_gb() - 8.0).abs() < 0.01);
        assert!(parse_nvidia_smi_csv("bad line").is_none());
    }

    #[test]
    fn select_gpu_adapter_sample_matches_discrete_gpu_on_multi_gpu() {
        // Simulate iGPU (128 MiB ded, 650 MiB shared) + dGPU (5400 MiB ded, 320 MiB shared)
        let samples = vec![(128, 650), (5400, 320)];
        assert_eq!(select_gpu_adapter_sample(&samples, Some(5480)), Some((5400, 320)));
        assert_eq!(select_gpu_adapter_sample(&samples, None), Some((5400, 320)));
        assert_eq!(select_gpu_adapter_sample(&[], Some(1000)), None);
    }

    #[test]
    fn local_http_client_polls_running_and_soft_unloads() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock swap");
        let port = listener.local_addr().expect("addr").port();

        let srv = thread::spawn(move || {
            for _ in 0..2 {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut req_buf = [0u8; 512];
                    let n = stream.read(&mut req_buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&req_buf[..n]);
                    if req.starts_with("GET /running") {
                        let body = r#"{"running":[{"model":"qwen3.6-35b","state":"ready"}]}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    } else if req.starts_with("GET /unload") {
                        let _ = stream.write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                        );
                    }
                }
            }
        });

        let running = poll_llama_swap_running("127.0.0.1", port).expect("poll /running");
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].model, "qwen3.6-35b");
        assert!(soft_unload_llama_swap("127.0.0.1", port).is_ok());
        let _ = srv.join();
    }
}
