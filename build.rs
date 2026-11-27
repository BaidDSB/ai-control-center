use std::path::{Path, PathBuf};
use std::process::Command;

fn find_windres_recursive(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !dir.exists() { return None; }
    if dir.join("windres.exe").exists() { return Some(dir.to_path_buf()); }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && let Some(found) = find_windres_recursive(&p, depth - 1)
            {
                return Some(found);
            }
        }
    }
    None
}

fn locate_windres_dir() -> Option<PathBuf> {
    if let Ok(local_app) = std::env::var("LOCALAPPDATA") {
        let links = Path::new(&local_app).join(r"Microsoft\WinGet\Links");
        if links.join("windres.exe").exists() { return Some(links); }
        let pkgs = Path::new(&local_app).join(r"Microsoft\WinGet\Packages");
        if let Some(found) = find_windres_recursive(&pkgs, 6) { return Some(found); }
    }
    for fixed in [r"C:\msys64\ucrt64\bin", r"C:\msys64\mingw64\bin", r"C:\mingw64\bin"] {
        let p = Path::new(fixed);
        if p.join("windres.exe").exists() { return Some(p.to_path_buf()); }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon_path = manifest_dir.join("assets").join("icon.ico");
    if !icon_path.exists() { return; }

    let Some(bin_dir) = locate_windres_dir() else { return; };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_file = out_dir.join("app_icon.rc");
    let obj_file = out_dir.join("app_icon.o");
    let icon_fwd = icon_path.to_string_lossy().replace('\\', "/");
    let rc_body = format!("1 ICON \"{icon_fwd}\"\n");
    if std::fs::write(&rc_file, rc_body).is_err() { return; }

    let current_path = std::env::var("PATH").unwrap_or_default();
    let full_path = format!("{};{}", bin_dir.display(), current_path);
    let windres_exe = bin_dir.join("windres.exe");
    let status = Command::new(&windres_exe)
        .env("PATH", &full_path)
        .args(["-O", "coff", rc_file.to_str().unwrap(), "-o", obj_file.to_str().unwrap()])
        .output();

    if let Ok(out) = status
        && out.status.success()
        && obj_file.exists()
    {
        println!("cargo:rustc-link-arg={}", obj_file.display());
    }
}