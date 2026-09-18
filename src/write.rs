use std::fs;

pub fn write_file(path: &str, content: &String) -> Result<String, String> {
    fs::write(path, content).map_err(|err| format!("Error {err} with file"))?;
    Ok(format!("wrote {} bytes to {path}", content.len()))
}
