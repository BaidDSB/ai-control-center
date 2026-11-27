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
    pub head_count: Option<u64>,
    pub head_count_kv: Option<u64>,
    pub embedding_length: Option<u64>,
    pub key_length: Option<u64>,
    pub value_length: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct VramEstimate {
    pub weights_vram_gb: f64,
    pub kv_cache_gb: f64,
    pub overhead_gb: f64,
    pub total_vram_gb: f64,
    pub total_ram_offload_gb: f64,
    pub is_approximate: bool,
}

fn kv_bytes_per_elem(kind: &str) -> f64 {
    match kind.trim().to_ascii_lowercase().as_str() {
        "f32" => 4.0,
        "f16" | "bf16" => 2.0,
        "q8_0" => 1.0625,
        "q5_0" | "q5_1" => 0.6875,
        "q4_0" | "q4_1" | "iq4_nl" | "turbo4" | "tq4" => 0.5625,
        "turbo3" | "tq3" => 0.4375,
        "turbo2" | "tq2" => 0.3125,
        _ => 1.0625,
    }
}

pub fn estimate_profile_vram(info: &GgufInfo, spec: &super::schema::ModelSpec) -> Option<VramEstimate> {
    if !info.valid || info.file_size_gb <= 0.0 {
        return None;
    }
    let layers = info.block_count.unwrap_or(32).max(1) as f64;
    let gpu_layer_frac = if spec.ngl_mode == "auto" {
        1.0
    } else {
        (spec.ngl_layers as f64 / layers).clamp(0.0, 1.0)
    };

    let mut weights_vram = info.file_size_gb * gpu_layer_frac;
    let mut ram_offload = (info.file_size_gb - weights_vram).max(0.0);

    let is_moe = info.expert_count.unwrap_or(0) > 1 || spec.moe_enabled;
    if is_moe {
        if spec.all_moe_cpu {
            let moved = weights_vram * 0.84;
            weights_vram -= moved;
            ram_offload += moved;
        } else if spec.moe_enabled && spec.n_cpu_moe > 0 {
            let off_frac = (spec.n_cpu_moe as f64 / layers).clamp(0.0, 1.0);
            let moved = weights_vram * 0.84 * off_frac;
            weights_vram -= moved;
            ram_offload += moved;
            if spec.moe_cache_mib > 0 {
                let cache_gb = spec.moe_cache_mib as f64 / 1024.0;
                weights_vram += cache_gb;
                ram_offload = (ram_offload - cache_gb).max(0.0);
            }
        }
    } else if spec.n_cpu_ffn > 0 {
        let off_frac = (spec.n_cpu_ffn as f64 / layers).clamp(0.0, 1.0);
        let moved = weights_vram * 0.65 * off_frac;
        weights_vram -= moved;
        ram_offload += moved;
    }

    let b_k = kv_bytes_per_elem(&spec.cache_type_k);
    let b_v = kv_bytes_per_elem(&spec.cache_type_v);
    let ctx = spec.ctx_size.max(512) as f64;
    let parallel = spec.parallel_slots.max(1) as f64;

    let (kv_total_gb, is_approximate) = if let Some(h_kv) = info.head_count_kv {
        let fallback_dim = match (info.embedding_length, info.head_count) {
            (Some(emb), Some(hc)) if hc > 0 => emb / hc,
            _ => 128,
        };
        let d_k = info.key_length.unwrap_or(fallback_dim) as f64;
        let d_v = info.value_length.unwrap_or(fallback_dim) as f64;
        let bytes = layers * ctx * parallel * (h_kv as f64) * (d_k * b_k + d_v * b_v);
        (bytes / (1024.0 * 1024.0 * 1024.0), false)
    } else {
        let approx_gb = 0.000125 * layers * (ctx / 1024.0) * parallel * ((b_k + b_v) * 0.5);
        (approx_gb, true)
    };

    let kv_vram_gb = if spec.no_kv_offload || gpu_layer_frac <= 0.0 {
        ram_offload += kv_total_gb;
        0.0
    } else {
        kv_total_gb
    };

    let overhead_gb = if gpu_layer_frac > 0.0 { 0.45 } else { 0.0 };
    let total_vram_gb = weights_vram + kv_vram_gb + overhead_gb;

    Some(VramEstimate {
        weights_vram_gb: weights_vram,
        kv_cache_gb: kv_vram_gb,
        overhead_gb,
        total_vram_gb,
        total_ram_offload_gb: ram_offload,
        is_approximate: is_approximate || spec.ngl_mode == "auto",
    })
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
        4..=6 => { r.seek(SeekFrom::Current(4)).ok()?; }
        10..=12 => { r.seek(SeekFrom::Current(8)).ok()?; }
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
        ..GgufInfo::default()
    };

    for _ in 0..kv_cnt.min(512) {
        let Some(key) = read_gguf_str(&mut r) else { break; };
        let Some(vtype) = read_u32_le(&mut r) else { break; };
        let is_uint = vtype == 4 || vtype == 10;
        if key == "general.architecture" && vtype == 8 {
            if let Some(s) = read_gguf_str(&mut r) { info.arch = s; } else { break; }
        } else if key.ends_with(".block_count") && is_uint {
            info.block_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".context_length") && is_uint {
            info.context_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".expert_count") && is_uint {
            info.expert_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".embedding_length") && is_uint {
            info.embedding_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".attention.head_count") && is_uint {
            info.head_count = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".attention.head_count_kv") && is_uint {
            info.head_count_kv = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".attention.key_length") && is_uint {
            info.key_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if key.ends_with(".attention.value_length") && is_uint {
            info.value_length = if vtype == 4 { read_u32_le(&mut r).map(|v| v as u64) } else { read_u64_le(&mut r) };
        } else if skip_gguf_val(&mut r, vtype).is_none() {
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
                } else if let Some(ext) = p.extension()
                    && ext.eq_ignore_ascii_case("gguf")
                        && let Ok(rel) = p.strip_prefix(base) {
                            found.push(rel.to_string_lossy().to_string());
                        }
            }
        }
    }
    found.sort();
    found
}
