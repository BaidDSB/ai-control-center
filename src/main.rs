#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
    let edition = if cfg!(feature = "plus") { "Plus" } else { "Core" };
    let patch = ai_control_center::AppPatch {
        app_title: Some(format!("AI Control Center {edition}")),
        ..Default::default()
    };
    ai_control_center::run_app(patch)
}