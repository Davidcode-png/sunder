use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    read::{list_dir, read_file},
    write::write_file,
};

#[derive(Debug)]
pub enum Tool {
    ReadFile {
        path: String,
        offset: Option<i64>,
        limit: Option<i64>,
    },
    ListDir {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
}

#[derive(Debug, Default)]
pub struct RawCall {
    pub id: String,
    pub name: String,
    pub args: String, // JSON text, built up fragment by fragment
}

impl Tool {
    pub fn run(&self) -> Result<String, String> {
        match self {
            Self::ReadFile {
                path,
                offset,
                limit,
            } => read_file(path, *offset, *limit),
            Self::ListDir { path } => list_dir(path),
            Self::WriteFile { path, content } => write_file(path, content),
        }
    }
}

/// Read an optional integer arg: missing/null -> None,
/// number -> itself, "42" -> 42, anything else -> None.
/// (Minion does `int(offset)` with a fallback to default, we just use None.)
fn opt_i64(v: &Value) -> Option<i64> {
    if v.is_null() {
        None
    } else if let Some(n) = v.as_i64() {
        Some(n)
    } else if let Some(n) = v.as_u64() {
        n.try_into().ok()
    } else if let Some(f) = v.as_f64() {
        // Ollama schema says "number", so the model may send 401.0
        Some(f as i64)
    } else {
        v.as_str()?.parse().ok()
    }
}

pub fn parse_tool(name: &str, args: &Value) -> Result<Tool, String> {
    match name {
        "read_file" => {
            let raw = args["path"].as_str().ok_or("Missing or invalid 'path'")?;
            // Bare-name fallback: the model often says "read.rs" when it means
            // "src/read.rs". If the path misses, retry under src/ before failing.
            let path = if std::path::Path::new(raw).exists() {
                raw.to_string()
            } else {
                let under_src = format!("src/{raw}");
                if !raw.contains('/') && std::path::Path::new(&under_src).exists() {
                    under_src
                } else {
                    raw.to_string()
                }
            };
            Ok(Tool::ReadFile {
                path,
                offset: opt_i64(&args["offset"]),
                limit: opt_i64(&args["limit"]),
            })
        }
        "list_dir" => {
            // minion: list_dir(path=".") — default when omitted/null.
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            Ok(Tool::ListDir {
                path: path.to_string(),
            })
        }
        "write_file" => {
            let path = args["path"].as_str().ok_or("Missing or invalid 'path'")?;
            let content = args["content"].as_str().ok_or("Invalid content")?;
            Ok(Tool::WriteFile {
                path: path.to_string(),
                content: content.to_string(),
            })
        }
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

pub fn accumulate(deltas: &[Value]) -> Vec<RawCall> {
    let mut calls: BTreeMap<usize, RawCall> = BTreeMap::new();
    // B2 fix: never unwrap index — role-only / content-only / usage-only
    // deltas have no "index". Assign them to the next open slot so a live
    // stream can't panic. Deltas with neither id/name/args are skipped.
    let mut next_auto: usize = 0;
    for delta in deltas {
        let has_payload = delta.get("id").and_then(|v| v.as_str()).is_some()
            || delta
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .is_some()
            || delta
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .is_some();
        if !has_payload {
            continue; // role-only, content-only, usage-only chunk
        }
        let index = match delta.get("index").and_then(|v| v.as_u64()) {
            Some(i) => i as usize,
            None => {
                // No index: attach to the slot currently being built, or open a new one.
                while calls.contains_key(&next_auto)
                    && !calls[&next_auto].args.is_empty()
                    && !calls[&next_auto].name.is_empty()
                {
                    next_auto += 1;
                }
                next_auto
            }
        };
        let call = calls.entry(index).or_insert_with(|| RawCall {
            id: String::new(),
            name: String::new(),
            args: String::new(),
        });
        if index >= next_auto && delta.get("index").is_none() {
            // keep auto cursor in sync
        }
        if let Some(id) = delta.get("id").and_then(|v| v.as_str()) {
            call.id = id.to_string();
        }
        if let Some(name) = delta.pointer("/function/name").and_then(|v| v.as_str()) {
            call.name = name.to_string();
        }

        if let Some(args) = delta
            .pointer("/function/arguments")
            .and_then(|v| v.as_str())
        {
            call.args.push_str(args);
        }
    }

    calls.into_values().collect()
}

pub fn build_tools(deltas: &[Value]) -> Vec<Result<Tool, String>> {
    let values = accumulate(deltas);
    let mut result = vec![];
    for value in values {
        let args: Value = match serde_json::from_str(&value.args) {
            Ok(args) => args,
            Err(e) => {
                result.push(Err(format!("Failed to parse tool arguments: {}", e)));
                continue;
            }
        };
        let parsed_tool = parse_tool(&value.name, &args);
        result.push(parsed_tool);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accumulate_skips_indexless_non_tool_deltas() {
        // role-only first chunk, content-only middle, usage-only last —
        // none of these may panic or produce phantom calls.
        let deltas = vec![
            json!({"role": "assistant"}),
            json!({"content": "hi"}),
            json!({"index": 0, "id": "a", "function": {"name": "read_file", "arguments": "{\"path\":"}}),
            json!({"index": 0, "function": {"arguments": "\"x\"}"}}),
            json!({"usage": {"prompt_tokens": 1}}),
        ];
        let calls = accumulate(&deltas);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].id, "a");
        let v: Value = serde_json::from_str(&calls[0].args).unwrap();
        assert_eq!(v["path"], json!("x"));
    }

    #[test]
    fn accumulate_indexless_tool_fragments_dont_panic() {
        let deltas = vec![json!({"id": "b", "function": {"name": "list_dir", "arguments": "{}"}})];
        let calls = accumulate(&deltas);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_dir");
    }

    #[test]
    fn parse_list_dir_defaults_to_dot() {
        let t = parse_tool("list_dir", &json!({})).unwrap();
        assert!(matches!(t, Tool::ListDir { path } if path == "."));
        let t = parse_tool("list_dir", &json!({"path": null})).unwrap();
        assert!(matches!(t, Tool::ListDir { path } if path == "."));
    }

    #[test]
    fn parse_unknown_tool_errors() {
        assert!(parse_tool("run_bash", &json!({"command": "ls"})).is_err());
    }
}
