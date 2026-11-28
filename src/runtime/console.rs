use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) const MAX_CONSOLE_LINES: usize = 3500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiNotifyTarget {
    Root,
    Console,
    Both,
}

pub type UiNotifier = Arc<dyn Fn(UiNotifyTarget) + Send + Sync>;

pub fn noop_notifier() -> UiNotifier {
    Arc::new(|_| {})
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
            if i > 0 {
                out.push('\n');
            }
            out.push_str(l);
        }
        out
    }
}

pub type SharedConsole = Arc<Mutex<ConsoleBuffer>>;
pub type SharedErrors = Arc<Mutex<Vec<String>>>;

pub fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for nc in chars.by_ref() {
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

pub(crate) fn is_critical_error_line(line_lower: &str) -> bool {
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

pub(crate) fn spawn_pipe_reader<R: Read + Send + 'static>(
    reader: R,
    prefix: &'static str,
    tag: &'static str,
    console: SharedConsole,
    errors: SharedErrors,
    notify: UiNotifier,
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
                    if cleaned.is_empty() {
                        continue;
                    }
                    let formatted = if prefix.is_empty() {
                        cleaned.clone()
                    } else {
                        format!("{prefix}{cleaned}")
                    };
                    if let Ok(mut c) = console.lock() {
                        c.push(formatted);
                    }
                    let lower = cleaned.to_ascii_lowercase();
                    if is_critical_error_line(&lower)
                        && last_err_logged.as_deref() != Some(cleaned.as_str())
                    {
                        last_err_logged = Some(cleaned.clone());
                        if let Ok(mut errs) = errors.lock() {
                            let num = errs.len() + 1;
                            errs.push(format!("[{tag} ERR #{num}] {cleaned}"));
                        }
                        notify(UiNotifyTarget::Both);
                    } else {
                        notify(UiNotifyTarget::Console);
                    }
                }
                Err(_) => break,
            }
        }
    });
}
