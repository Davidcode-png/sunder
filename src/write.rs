use std::fs;
use std::io::{self, Write};


/// Sunder's confirm is deliberately thin: YOLO env (`SUNDER_YOLO=1`, or
/// `--yolo`) skips the prompt; otherwise print `allow <action>? [Y/n]` and
/// read one line from stdin. `n`/`no` denies, anything else allows.

pub fn confirm(action: &str) -> bool {
    if std::env::var("SUNDER_YOLO").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
        return true;
    }
    print!("allow {action}? [Y/n] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    !matches!(line.trim().to_lowercase().as_str(), "n" | "no")
}

pub fn write_file(path: &str, content: &str) -> Result<String, String> {
    if !confirm(&format!("write {path} ({} bytes)", content.len())) {
        return Ok("DENIED by user".to_string());
    }
    fs::write(path, content).map_err(|err| format!("Error {err} with file"))?;
    Ok(format!("wrote {} bytes to {path}", content.len()))
}

