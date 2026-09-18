use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    read::{list_dir, read_file},
    write::write_file,
};

#[derive(Debug)]
pub enum Tool {
    ReadFile { path: String },
    ListDir { path: String },
    WriteFile { path: String, content: String },
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
            Self::ReadFile { path } => read_file(path),
            Self::ListDir { path } => list_dir(path),
            Self::WriteFile { path, content } => write_file(path, content),
        }
    }
}

pub fn parse_tool(name: &str, args: &Value) -> Result<Tool, String> {
    match name {
        "read_file" => {
            let path = args["path"].as_str().ok_or("Missing or invalid 'path'")?;
            Ok(Tool::ReadFile {
                path: path.to_string(),
            })
        }
        "list_dir" => {
            let path = args["path"].as_str().ok_or("Missing or invalid 'path'")?;
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
    for delta in deltas {
        let index = delta["index"].as_u64().unwrap() as usize;
        let call = calls.entry(index).or_insert_with(|| RawCall {
            id: String::new(),
            name: String::new(),
            args: String::new(),
        });
        if let Some(id) = delta["id"].as_str() {
            call.id = id.to_string();
        }
        if let Some(name) = delta["function"]["name"].as_str() {
            call.name = name.to_string();
        }

        if let Some(args) = delta["function"]["arguments"].as_str() {
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
