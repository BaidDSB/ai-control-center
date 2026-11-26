use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct GgufInfo {
    pub valid: bool,
    pub file_size_gb: f64,
    pub arch: String,
    pub block_count: Option<u64>,
    pub context_length: Option<u64>,
    pub expert_count: Option<u64>,
}

fn read_u32_le<R: Read>(r: &mut R) -> Option<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b))
}

fn read_u64_le<R: Read>(r: &mut R) -> Option<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

fn read_gguf_str<R: Read>(r: &mut R) -> Option<String> {
    let len = read_u64_le(r)? as usize;
    if len > 4096 { return None; }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn skip_gguf_val<R: Read + Seek>(r: &mut R, vtype: u32) -> Option<()> {
    match vtype {
        0 | 1 | 7 => { r.seek(SeekFrom::Current(1)).ok()?; }
        2 | 3 => { r.seek(SeekFrom::Current(2)).ok()?; }
        4 | 5 | 6 => { r.seek(SeekFrom::Current(4)).ok()?; }
        10 | 11 | 12 => { r.seek(SeekFrom::Current(8)).ok()?; }
        8 => {
            let len = read_u64_le(r)? as i64;
            r.seek(SeekFrom::Current(len)).ok()?;
        }
        9 => {
            let elem_type = read_u32_le(r)?;
            let count = read_u64_le(r)?;
            if count > 200_000 { return None; }
            for _ in 0..count { skip_gguf_val(r, elem_type)?; }
        }
        _ => return None,
    }
    Some(())
}

pub fn probe_gguf_header(models_dir: &str, rel_path: &str) -> GgufInfo {
    let full = Path::new(models_dir).join(rel_path);
    let Ok(meta) = fs::metadata(&full) else { return GgufInfo::default(); };
    let size_gb = meta.len() as f64 / (1024.0 * 1024.0 * 1024.0);
    let Ok(file) = File::open(&full) else { return GgufInfo::default(); };
    let mut r = BufReader::new(file);
    let mut magic = [0u8; 4];
    if r.read_exact(&mut magic).is_err() || &magic != b"GGUF" { return GgufInfo::default(); }
    let Some(ver) = read_u32_le(&mut r) else { return GgufInfo::default(); };
    if !(2..=3).contains(&ver) { return GgufInfo::default(); }
    let Some(_t) = read_u64_le(&mut r) else { return GgufInfo::default(); };
    let Some(kv_cnt) = read_u64_le(&mut r) else { return GgufInfo::default(); };

    let mut info = GgufInfo {
        valid: true,
        file_size_gb: size_gb,
        arch: "unknown".to_string(),
        block_count: None,
        context_length: None,
        expert_count: None,
    };

    for _ in 0..kv_cnt.min(512) {
        let Some(key) = read_gguf_str(&mut r) else { break; };
        let Some(vtype) = read_u32_le(&mut r) else { break; };
        if key == "general.architecture" && vtype == 8 {
            if let Some(s) = read_gguf_str(&mut r) { info.arch = s; } else { break; }
        } else if key.ends_with(".block_count") && (vtype == 4 || vtype == 10) {
            info.block_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".context_length") && (vtype == 4 || vtype == 10) {
            info.context_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".expert_count") && (vtype == 4 || vtype == 10) {
            info.expert_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if skip_gguf_val(&mut r, vtype).is_none() {
            break;
        }
        if info.arch != "unknown"
            && info.block_count.is_some()
            && info.context_length.is_some()
            && info.expert_count.is_some()
        {
            break;
        }
    }
    info
}

pub fn scan_gguf_models(root: &str) -> Vec<String> {
    let base = Path::new(root);
    let mut found = Vec::new();
    if !base.exists() { return found; }
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(ext) = p.extension() {
                    if ext.eq_ignore_ascii_case("gguf") {
                        if let Ok(rel) = p.strip_prefix(base) {
                            found.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    found.sort();
    found
}
