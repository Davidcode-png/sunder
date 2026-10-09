use std::fs;

use crate::write::confirm;

/// Returns a `cat -n` style window so big files don't blow the context.
/// `offset` is 1-based, `limit` is max lines (None = 400, <=0 = to end).
pub fn read_file(path: &str, offset: Option<i64>, limit: Option<i64>) -> Result<String, String> {
    let content =
        fs::read_to_string(path).map_err(|err| format!("File cannot be found {err}"))?;
    // split_inclusive keeps the '\n' on each line, like Python's readlines()
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let total = lines.len();

    if total == 0 {
        return Ok(format!("[{path}: empty file]"));
    }

    // minion: start = max(1, int(offset)) - 1
    let start = offset.unwrap_or(1).max(1) as usize - 1;
    if start >= total {
        return Ok(format!(
            "[{path}: {total} lines; offset {} is past end of file]",
            start + 1
        ));
    }

    // minion: limit None -> MINION_READ_FILE_LINES (400)
    let limit = limit.unwrap_or(400);
    let end = if limit <= 0 {
        total
    } else {
        (start + limit as usize).min(total)
    };

    let mut body = String::new();
    for (i, line) in lines.iter().enumerate().take(end).skip(start) {
        // "{i+1:6d}\t{line}" — right-aligned number + tab, exactly like minion
        body.push_str(&format!("{:6}\t{}", i + 1, line));
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }

    // Only add the header when this is a partial window
    if start > 0 || end < total {
        Ok(format!(
            "[{path}: lines {}-{end} of {total}; call read_file with offset/limit to page]\n{body}",
            start + 1
        ))
    } else {
        Ok(body)
    }
}


pub fn list_dir(path: &str) -> Result<String, String> {
    let directory =
        fs::read_dir(path).map_err(|err| format!("Path cannot be found, {err}"))?;
    let mut names: Vec<String> = Vec::new();
    for entry in directory {
        let entry = entry.map_err(|err| format!("Failed to read entry, {err}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Trailing '/' signals "this is a folder you can list_dir into".
        // Without it the model can't tell files from folders (ROADMAP D3).
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false);
        if is_dir {
            names.push(format!("{name}/"));
        } else {
            names.push(name);
        }
    }
    names.sort();
    Ok(names.join("\n"))
}

/// Port of minion.py:_strip_line_numbers (line 1504): remove read_file's
/// `<n>\t` prefixes only when EVERY non-empty line carries one — so pasted
/// numbered output is cleaned but ordinary code starting with digits is kept.
pub fn strip_line_numbers(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let nonempty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    let all_numbered = !nonempty.is_empty() && nonempty.iter().all(|l| {
        let mut chars = l.chars();
        let mut saw_digit = false;
        for c in chars.by_ref() {
            if c.is_ascii_digit() {
                saw_digit = true;
            } else if c == '\t' {
                break;
            } else if c == ' ' && saw_digit {
                continue;
            } else {
                return false;
            }
        }
        // must have ended on a tab after digits (mimics `^ *\d+\t`)
        saw_digit && l.contains('\t') && {
            let prefix = l.split('\t').next().unwrap_or("");
            !prefix.is_empty() && prefix.trim().chars().all(|c| c.is_ascii_digit())
        }
    });
    if !all_numbered {
        return text.to_string();
    }
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                (*l).to_string()
            } else if let Some(pos) = l.find('\t') {
                l[pos + 1..].to_string()
            } else {
                (*l).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Port of minion.py:edit_file (line 1516): `old` must match EXACTLY once.
/// Falls back to stripped line numbers (model pasted read_file output), then
/// gates on confirm. Wrinkle from minion: edit rewrites with the line ending
/// style the file already uses.
pub fn edit_file(path: &str, old: &str, new: &str) -> Result<String, String> {
    let src = fs::read_to_string(path).map_err(|err| format!("Error {err} with file"))?;
    let mut old_s = old.to_string();
    let mut new_s = new.to_string();
    if src.matches(&old_s).count() != 1 {
        let stripped_old = strip_line_numbers(old);
        if stripped_old != old_s && src.matches(&stripped_old).count() == 1 {
            old_s = stripped_old;
            new_s = strip_line_numbers(new);
        }
    }
    let count = src.matches(&old_s).count();
    if count != 1 {
        return Ok(format!("ERROR: `old` matched {count} times (need exactly 1)"));
    }
    if !confirm(&format!("edit {path}")) {
        return Ok("DENIED by user".to_string());
    }
    fs::write(path, src.replacen(&old_s, &new_s, 1))
        .map_err(|err| format!("Error {err} with file"))?;
    Ok(format!("edited {path}"))
}



#[cfg(test)]
mod read_tests {
    use super::*;
    use std::fs;

    #[test]
    fn strip_numbered_block() {
        let numbered = "     1\thello\n     2\tworld";
        assert_eq!(strip_line_numbers(numbered), "hello\nworld");
    }

    #[test]
    fn strip_leaves_normal_code_alone() {
        let code = "123 = bad idea\nnormal line";
        assert_eq!(strip_line_numbers(code), code);
    }

    #[test]
    fn edit_requires_exactly_one_match() {
        unsafe { std::env::set_var("SUNDER_YOLO", "1") };
        let p = "/tmp/sunder_edit_test.txt";
        fs::write(p, "aaa bbb aaa").unwrap();
        let r = edit_file(p, "aaa", "zzz").unwrap();
        assert!(r.contains("matched 2 times"), "{r}");
        fs::write(p, "hello world").unwrap();
        assert_eq!(edit_file(p, "world", "there").unwrap(), format!("edited {p}"));
        assert_eq!(fs::read_to_string(p).unwrap(), "hello there");
        let _ = fs::remove_file(p);
    }
}
