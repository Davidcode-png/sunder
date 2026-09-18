use std::fs;

pub fn read_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("File cannot be found {}", err))
}

pub fn list_dir(path: &str) -> Result<String, String> {
    let directory = fs::read_dir(path).map_err(|err| format!("Path cannot be found, {}", err))?;
    let mut output = String::new();
    for entry in directory {
        let entry = entry.map_err(|err| format!("Failed to read entry, {}", err))?;
        let file_type = entry
            .file_type()
            .map_err(|err| format!("Failed to get file type, {err}"))?;
        let type_str = if file_type.is_dir() { "Dir " } else { "File" };
        output.push_str(&format!(
            "{} : {}\n",
            type_str,
            entry.file_name().to_string_lossy()
        ));
    }
    Ok(output)
}

// pub fn read_file_fast(path: &String) {}
