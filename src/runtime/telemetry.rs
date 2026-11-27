use super::state::normalize_probe_host;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunningModelInfo {
    pub model: String,
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuVramInfo {
    pub used_mib: u64,
    pub total_mib: u64,
    pub shared_used_mib: Option<u64>,
    pub shared_total_mib: Option<u64>,
    pub sys_ram_used_mib: Option<u64>,
    pub sys_ram_total_mib: Option<u64>,
    pub name: String,
}

impl GpuVramInfo {
    pub fn used_gb(&self) -> f64 {
        self.used_mib as f64 / 1024.0
    }

    pub fn total_gb(&self) -> f64 {
        self.total_mib as f64 / 1024.0
    }

    pub fn usage_ratio(&self) -> f64 {
        if self.total_mib == 0 {
            0.0
        } else {
            (self.used_mib as f64 / self.total_mib as f64).clamp(0.0, 1.0)
        }
    }

    pub fn shared_used_gb(&self) -> Option<f64> {
        self.shared_used_mib.map(|m| m as f64 / 1024.0)
    }

    pub fn shared_total_gb(&self) -> Option<f64> {
        self.shared_total_mib.map(|m| m as f64 / 1024.0)
    }

    pub fn sys_ram_used_gb(&self) -> Option<f64> {
        self.sys_ram_used_mib.map(|m| m as f64 / 1024.0)
    }

    pub fn sys_ram_total_gb(&self) -> Option<f64> {
        self.sys_ram_total_mib.map(|m| m as f64 / 1024.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TelemetrySnapshot {
    pub swap_reachable: bool,
    pub running_models: Vec<RunningModelInfo>,
    pub gpu_vram: Option<GpuVramInfo>,
}

fn decode_chunked_body(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    loop {
        let Some((size_line, after_size)) = rest.split_once("\r\n") else {
            return raw.to_string();
        };
        let hex = size_line.split(';').next().unwrap_or("").trim();
        let Ok(chunk_len) = usize::from_str_radix(hex, 16) else {
            return raw.to_string();
        };
        if chunk_len == 0 {
            break;
        }
        if after_size.len() < chunk_len {
            out.push_str(after_size);
            break;
        }
        out.push_str(&after_size[..chunk_len]);
        rest = after_size[chunk_len..]
            .strip_prefix("\r\n")
            .unwrap_or(&after_size[chunk_len..]);
    }
    out
}

pub fn http_local_request(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    timeout_ms: u64,
) -> Result<(u16, String), String> {
    if port == 0 {
        return Err("Invalid port 0".to_string());
    }
    let probe_host = normalize_probe_host(host);
    let addr_str = if probe_host.contains(':') && !probe_host.starts_with('[') {
        format!("[{probe_host}]:{port}")
    } else {
        format!("{probe_host}:{port}")
    };
    let timeout = Duration::from_millis(timeout_ms.max(50));
    let mut addrs = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("Address resolution failed for {addr_str}: {e}"))?;
    let sock_addr = addrs
        .next()
        .ok_or_else(|| format!("No socket address resolved for {addr_str}"))?;

    let mut stream = TcpStream::connect_timeout(&sock_addr, timeout)
        .map_err(|e| format!("Connect failed ({addr_str}): {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {probe_host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("HTTP write failed: {e}"))?;

    let mut buf = Vec::new();
    let _ = stream.take(262_144).read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf);
    let (hdr_part, body_part) = if let Some((h, b)) = raw.split_once("\r\n\r\n") {
        (h, b)
    } else if let Some((h, b)) = raw.split_once("\n\n") {
        (h, b)
    } else {
        return Err("Malformed HTTP response".to_string());
    };

    let status_line = hdr_part.lines().next().unwrap_or("");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("Invalid HTTP status line: {status_line}"))?;

    let is_chunked = hdr_part.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let body = if is_chunked {
        decode_chunked_body(body_part)
    } else {
        body_part.to_string()
    };
    Ok((status, body))
}

fn read_json_string(bytes: &[u8], start_quote: usize) -> Option<(String, usize)> {
    if bytes.get(start_quote).copied() != Some(b'"') {
        return None;
    }
    let mut out = String::new();
    let mut i = start_quote + 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && i + 1 < bytes.len() {
            let esc = bytes[i + 1];
            match esc {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                other => out.push(other as char),
            }
            i += 2;
        } else if c == b'"' {
            return Some((out, i + 1));
        } else {
            let s = std::str::from_utf8(&bytes[i..]).ok()?;
            let ch = s.chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    None
}

fn extract_json_str_field(obj: &str, key: &str) -> Option<String> {
    let bytes = obj.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if let Some((tok, next_i)) = read_json_string(bytes, i) {
                i = next_i;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b':' {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if tok == key && i < bytes.len() && bytes[i] == b'"' {
                        return read_json_string(bytes, i).map(|(v, _)| v);
                    }
                }
                continue;
            }
        }
        i += 1;
    }
    None
}

pub fn parse_llama_swap_running_json(body: &str) -> Option<Vec<RunningModelInfo>> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let arr_start = if bytes[0] == b'[' {
        0
    } else {
        let key_pos = trimmed.find("\"running\"")?;
        let rel_bracket = trimmed[key_pos..].find('[')?;
        key_pos + rel_bracket
    };

    let mut depth_arr = 0usize;
    let mut depth_obj = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut obj_start: Option<usize> = None;
    let mut out = Vec::new();

    for (idx, &b) in bytes.iter().enumerate().skip(arr_start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' => depth_arr += 1,
            b']' => {
                depth_arr = depth_arr.saturating_sub(1);
                if depth_arr == 0 {
                    break;
                }
            }
            b'{' => {
                if depth_arr == 1 && depth_obj == 0 {
                    obj_start = Some(idx);
                }
                depth_obj += 1;
            }
            b'}' => {
                depth_obj = depth_obj.saturating_sub(1);
                if depth_arr == 1 && depth_obj == 0 {
                    if let Some(s) = obj_start.take() {
                        let slice = &trimmed[s..=idx];
                        let model = extract_json_str_field(slice, "model")
                            .or_else(|| extract_json_str_field(slice, "id"))
                            .or_else(|| extract_json_str_field(slice, "name"));
                        if let Some(m) = model.filter(|v| !v.trim().is_empty()) {
                            let state = extract_json_str_field(slice, "state")
                                .unwrap_or_else(|| "running".to_string());
                            out.push(RunningModelInfo { model: m, state });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Some(out)
}

pub fn poll_llama_swap_running(host: &str, port: u16) -> Option<Vec<RunningModelInfo>> {
    let (status, body) = http_local_request(host, port, "GET", "/running", 350).ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    parse_llama_swap_running_json(&body)
}

pub fn soft_unload_llama_swap(host: &str, port: u16) -> Result<(), String> {
    let (status, body) = http_local_request(host, port, "GET", "/unload", 1500)?;
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status == 405 {
        let (st2, body2) = http_local_request(host, port, "POST", "/unload", 1500)?;
        if (200..300).contains(&st2) {
            return Ok(());
        }
        return Err(format!("POST /unload returned HTTP {st2}: {}", body2.trim()));
    }
    Err(format!("GET /unload returned HTTP {status}: {}", body.trim()))
}

pub fn llama_swap_ui_url(host: &str, port: u16) -> String {
    let h = normalize_probe_host(host);
    format!("http://{h}:{port}/ui")
}

pub fn parse_nvidia_smi_csv(stdout: &str) -> Option<GpuVramInfo> {
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
        if parts.len() >= 2 {
            let used = parts[0].parse::<u64>().ok()?;
            let total = parts[1].parse::<u64>().ok()?;
            if total == 0 {
                continue;
            }
            let name = parts
                .get(2)
                .map(|s| s.to_string())
                .unwrap_or_else(|| "NVIDIA GPU".to_string());
            return Some(GpuVramInfo {
                used_mib: used,
                total_mib: total,
                shared_used_mib: None,
                shared_total_mib: None,
                sys_ram_used_mib: None,
                sys_ram_total_mib: None,
                name,
            });
        }
    }
    None
}

pub fn select_gpu_adapter_sample(
    samples: &[(u64, u64)],
    hint_dedicated_mib: Option<u64>,
) -> Option<(u64, u64)> {
    if samples.is_empty() {
        return None;
    }
    if let Some(target) = hint_dedicated_mib {
        let with_ded: Vec<(u64, u64)> = samples
            .iter()
            .copied()
            .filter(|&(d, _)| d > 0)
            .collect();
        let pool = if with_ded.is_empty() { samples } else { &with_ded };
        return pool
            .iter()
            .copied()
            .min_by_key(|&(d, s)| (d.abs_diff(target), std::cmp::Reverse(s)));
    }
    samples
        .iter()
        .copied()
        .max_by_key(|&(d, s)| (d, s))
}

#[cfg(windows)]
fn probe_windows_system_ram_mib() -> Option<(u64, u64)> {
    #[repr(C)]
    struct MemoryStatusEx {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut st = MemoryStatusEx {
        dw_length: std::mem::size_of::<MemoryStatusEx>() as u32,
        dw_memory_load: 0,
        ull_total_phys: 0,
        ull_avail_phys: 0,
        ull_total_page_file: 0,
        ull_avail_page_file: 0,
        ull_total_virtual: 0,
        ull_avail_virtual: 0,
        ull_avail_extended_virtual: 0,
    };
    let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
    if ok == 0 || st.ull_total_phys == 0 {
        return None;
    }
    let total_mib = st.ull_total_phys / (1024 * 1024);
    let used_bytes = st.ull_total_phys.saturating_sub(st.ull_avail_phys);
    let used_mib = used_bytes / (1024 * 1024);
    Some((used_mib, total_mib))
}

#[cfg(windows)]
fn probe_windows_pdh_gpu_memory_mib(hint_dedicated_mib: Option<u64>) -> Option<(u64, u64)> {
    use std::collections::HashMap;
    use std::ffi::c_void;

    type PdhHQuery = *mut c_void;
    type PdhHCounter = *mut c_void;

    const PDH_FMT_LARGE: u32 = 0x0000_0200;
    const PDH_MORE_DATA: u32 = 0x8000_07D2;

    #[repr(C)]
    struct PdhFmtCounterValue {
        c_status: u32,
        large_value: i64,
    }

    #[repr(C)]
    struct PdhFmtCounterValueItemW {
        sz_name: *mut u16,
        fmt_value: PdhFmtCounterValue,
    }

    #[link(name = "pdh")]
    unsafe extern "system" {
        fn PdhOpenQueryW(
            sz_data_source: *const u16,
            dw_user_data: usize,
            ph_query: *mut PdhHQuery,
        ) -> u32;
        fn PdhAddEnglishCounterW(
            h_query: PdhHQuery,
            sz_full_counter_path: *const u16,
            dw_user_data: usize,
            ph_counter: *mut PdhHCounter,
        ) -> u32;
        fn PdhCollectQueryData(h_query: PdhHQuery) -> u32;
        fn PdhGetFormattedCounterArrayW(
            h_counter: PdhHCounter,
            dw_format: u32,
            lpdw_buffer_size: *mut u32,
            lpdw_item_count: *mut u32,
            item_buffer: *mut PdhFmtCounterValueItemW,
        ) -> u32;
        fn PdhCloseQuery(h_query: PdhHQuery) -> u32;
    }

    struct QueryGuard(PdhHQuery);
    impl Drop for QueryGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    PdhCloseQuery(self.0);
                }
            }
        }
    }

    fn to_wide_nul(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_wide_name(ptr: *const u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        unsafe {
            while len < 512 && *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf16_lossy(slice)
        }
    }

    fn read_counter_array(counter: PdhHCounter) -> Vec<(String, u64)> {
        let mut buf_size: u32 = 0;
        let mut item_count: u32 = 0;
        let st = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut buf_size,
                &mut item_count,
                std::ptr::null_mut(),
            )
        };
        if st != PDH_MORE_DATA || buf_size == 0 || item_count == 0 {
            return Vec::new();
        }
        // Allocate u64-aligned storage for PDH_FMT_COUNTERVALUE_ITEM_W array + trailing strings
        let mut raw = vec![0u64; (buf_size as usize).div_ceil(8)];
        let items_ptr = raw.as_mut_ptr() as *mut PdhFmtCounterValueItemW;
        let st2 = unsafe {
            PdhGetFormattedCounterArrayW(
                counter,
                PDH_FMT_LARGE,
                &mut buf_size,
                &mut item_count,
                items_ptr,
            )
        };
        if st2 != 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(item_count as usize);
        for i in 0..(item_count as usize) {
            let item = unsafe { &*items_ptr.add(i) };
            if item.fmt_value.c_status <= 1 {
                let name = read_wide_name(item.sz_name);
                let mib = item.fmt_value.large_value.max(0) as u64 / (1024 * 1024);
                out.push((name, mib));
            }
        }
        out
    }

    let mut query: PdhHQuery = std::ptr::null_mut();
    if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 || query.is_null() {
        return None;
    }
    let _guard = QueryGuard(query);

    let ded_path = to_wide_nul(r"\GPU Adapter Memory(*)\Dedicated Usage");
    let shd_path = to_wide_nul(r"\GPU Adapter Memory(*)\Shared Usage");
    let mut ded_counter: PdhHCounter = std::ptr::null_mut();
    let mut shd_counter: PdhHCounter = std::ptr::null_mut();

    let has_ded = unsafe { PdhAddEnglishCounterW(query, ded_path.as_ptr(), 0, &mut ded_counter) } == 0;
    let has_shd = unsafe { PdhAddEnglishCounterW(query, shd_path.as_ptr(), 0, &mut shd_counter) } == 0;
    if !has_ded && !has_shd {
        return None;
    }
    if unsafe { PdhCollectQueryData(query) } != 0 {
        return None;
    }

    let mut by_adapter: HashMap<String, (u64, u64)> = HashMap::new();
    if has_ded {
        for (name, mib) in read_counter_array(ded_counter) {
            by_adapter.entry(name).or_default().0 = mib;
        }
    }
    if has_shd {
        for (name, mib) in read_counter_array(shd_counter) {
            by_adapter.entry(name).or_default().1 = mib;
        }
    }

    let samples: Vec<(u64, u64)> = by_adapter.into_values().collect();
    select_gpu_adapter_sample(&samples, hint_dedicated_mib)
}

#[cfg(target_os = "linux")]
fn probe_linux_meminfo_mib() -> Option<(u64, u64)> {
    let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib: Option<u64> = None;
    let mut avail_kib: Option<u64> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kib = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kib = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok());
        }
    }
    let total = total_kib?;
    let avail = avail_kib.unwrap_or(0);
    Some((total.saturating_sub(avail) / 1024, total / 1024))
}

#[cfg(target_os = "linux")]
fn probe_linux_drm_gpu() -> Option<GpuVramInfo> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in entries.flatten() {
        let dev = entry.path().join("device");
        let used_b = std::fs::read_to_string(dev.join("mem_info_vram_used"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let total_b = std::fs::read_to_string(dev.join("mem_info_vram_total"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        if let (Some(u), Some(t)) = (used_b, total_b) {
            if t == 0 {
                continue;
            }
            let gtt_used = std::fs::read_to_string(dev.join("mem_info_gtt_used"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|b| b / (1024 * 1024));
            let gtt_total = std::fs::read_to_string(dev.join("mem_info_gtt_total"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|b| b / (1024 * 1024));
            return Some(GpuVramInfo {
                used_mib: u / (1024 * 1024),
                total_mib: t / (1024 * 1024),
                shared_used_mib: gtt_used,
                shared_total_mib: gtt_total,
                sys_ram_used_mib: None,
                sys_ram_total_mib: None,
                name: "Linux DRM GPU".to_string(),
            });
        }
    }
    None
}

fn probe_nvidia_smi() -> Option<GpuVramInfo> {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=memory.used,memory.total,name",
        "--format=csv,noheader,nounits",
    ])
    .stdin(Stdio::null())
    .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_csv(&stdout)
}

pub fn probe_local_gpu_vram() -> Option<GpuVramInfo> {
    let mut info = probe_nvidia_smi();

    #[cfg(windows)]
    {
        let sys_ram = probe_windows_system_ram_mib();
        let hint_ded = info.as_ref().map(|g| g.used_mib);
        let pdh = probe_windows_pdh_gpu_memory_mib(hint_ded);
        let shared_cap_mib = sys_ram.map(|(_, total)| total / 2);

        match (&mut info, pdh) {
            (Some(gpu), Some((_, shared_used))) => {
                gpu.shared_used_mib = Some(shared_used);
                gpu.shared_total_mib = shared_cap_mib;
            }
            (None, Some((ded_used, shared_used))) => {
                info = Some(GpuVramInfo {
                    used_mib: ded_used,
                    total_mib: 0,
                    shared_used_mib: Some(shared_used),
                    shared_total_mib: shared_cap_mib,
                    sys_ram_used_mib: None,
                    sys_ram_total_mib: None,
                    name: "Local GPU (WDDM)".to_string(),
                });
            }
            _ => {}
        }

        if let (Some(gpu), Some((ram_used, ram_total))) = (&mut info, sys_ram) {
            gpu.sys_ram_used_mib = Some(ram_used);
            gpu.sys_ram_total_mib = Some(ram_total);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if info.is_none() {
            info = probe_linux_drm_gpu();
        }
        if let (Some(gpu), Some((ram_used, ram_total))) = (&mut info, probe_linux_meminfo_mib()) {
            gpu.sys_ram_used_mib = Some(ram_used);
            gpu.sys_ram_total_mib = Some(ram_total);
        }
    }

    info
}

pub fn poll_telemetry(host: &str, port: u16, backend_running: bool) -> TelemetrySnapshot {
    let (swap_reachable, running_models) = if backend_running {
        match poll_llama_swap_running(host, port) {
            Some(models) => (true, models),
            None => (false, Vec::new()),
        }
    } else {
        (false, Vec::new())
    };
    let gpu_vram = probe_local_gpu_vram();
    TelemetrySnapshot {
        swap_reachable,
        running_models,
        gpu_vram,
    }
}
