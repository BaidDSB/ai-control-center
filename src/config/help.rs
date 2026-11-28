use super::schema::slugify;
use std::collections::HashMap;
use std::fs;

pub fn guess_help_key(name: &str, model_file: &str) -> String {
    let hay = format!("{} {}", name, model_file).to_ascii_lowercase();
    if hay.contains("3.6") || hay.contains("35b") { "qwen3.6-35b".to_string() }
    else if hay.contains("gemma") { "gemma4-26b".to_string() }
    else if hay.contains("3.8") || hay.contains("27b") { "qwen3.8-27b".to_string() }
    else if hay.contains("9b") || hay.contains("defiant") { "qwen3.5-9b".to_string() }
    else if hay.contains("4b") { "qwen3.5-4b".to_string() }
    else if hay.contains("cydonia") || hay.contains("magnum") { "cydonia-24b".to_string() }
    else if hay.contains("huihui") { "huihui-24b".to_string() }
    else { slugify(name) }
}

pub type HelpMap = HashMap<(String, String, String), String>;

pub fn parse_help_markdown(path: &str) -> HelpMap {
    let Ok(content) = fs::read_to_string(path) else { return HashMap::new(); };
    parse_help_markdown_str(&content)
}

pub fn parse_help_markdown_str(content: &str) -> HelpMap {
    let mut map = HashMap::new();
    let mut current_key: Option<(String, String, String)> = None;
    let mut buf = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<!--") && trimmed.ends_with("-->") && trimmed.contains("@model:") {
            if let Some(k) = current_key.take() {
                map.insert(k, buf.join("\n").trim().to_string());
                buf.clear();
            }
            let inner = trimmed.trim_start_matches("<!--").trim_end_matches("-->").trim();
            let mut model = String::new();
            let mut engine = "general".to_string();
            let mut variant = String::new();
            for part in inner.split('|') {
                let p = part.trim();
                if let Some(v) = p.strip_prefix("@model:") { model = v.trim().to_ascii_lowercase(); }
                else if let Some(v) = p.strip_prefix("@engine:") { engine = v.trim().to_ascii_lowercase(); }
                else if let Some(v) = p.strip_prefix("@variant:") { variant = v.trim().to_ascii_lowercase(); }
            }
            if !model.is_empty() {
                current_key = Some((model, engine, variant));
            }
        } else if current_key.is_some() {
            buf.push(line);
        }
    }
    if let Some(k) = current_key.take() {
        map.insert(k, buf.join("\n").trim().to_string());
    }
    map
}
