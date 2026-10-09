use futures_util::StreamExt;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, Write};

use crate::tool::parse_tool;

// --- Ollama /api/chat shapes -------------------------------------------
// Request:  { model, messages, stream, tools: [{type, function:{name, description, parameters}}] }
// Stream:   NDJSON lines like {"message": {"role":"assistant","content":"...","tool_calls":[...]}, "done":false}
// Tool echo: assistant msg with tool_calls, then {"role":"tool","tool_name":...,"content":...}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ToolDefinition {
    r#type: String,
    function: FunctionDefinition,
}

/// `path` is required, `offset`/`limit` are optional (our paging from last step).
/// Description mirrors minion.py TOOLS[read_file] (shortened, paging kept).
fn read_file_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "read_file".to_string(),
            description: "Read a file's contents. Returns lines numbered (1-based, like `cat -n`). Large files return only a window — pass `offset` (1-based start line) and `limit` (max lines, default 400) to page; a header shows the visible range and total line count.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path":   { "type": "string" },
                    "offset": { "type": "integer", "description": "1-based line to start from (default 1)" },
                    "limit":  { "type": "integer", "description": "max lines to return (default 400; <=0 reads to end)" }
                }
            }),
        },
    }
}

fn list_dir_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "list_dir".to_string(),
            description: "List a directory (names ending in '/' are subfolders — call list_dir on them to go deeper, then read_file on files you find).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to list (default \".\")" }
                }
            }),
        },
    }
}

fn write_file_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "write_file".to_string(),
            description: "Write (overwrite) a file".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path":    { "type": "string" },
                    "content": { "type": "string" }
                }
            }),
        },
    }
}

fn edit_file_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "edit_file".to_string(),
            description: "Replace one exact occurrence of `old` with `new` in a file. Prefer raw file text; if you paste read_file numbered lines, the line-number prefixes are stripped automatically.".to_string(),
            parameters: json!({"type": "object", "required": ["path", "old", "new"],
                "properties": {"path": {"type": "string"}, "old": {"type": "string"}, "new": {"type": "string"}}}),
        },
    }
}

fn run_bash_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "run_bash".to_string(),
            description: "Run a shell command. Quick commands return output directly; long ones keep running in the background and return a PID + log path (check with wait_background or read_file). Do NOT use sleep N as a chaining trick.".to_string(),
            parameters: json!({"type": "object", "required": ["command"],
                "properties": {"command": {"type": "string"},
                "timeout": {"type": "integer", "description": "Seconds to wait synchronously (default ~3; 0 = wait indefinitely). Only set when you KNOW the command finishes in ~N seconds."}}}),
        },
    }
}

fn wait_background_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDefinition {
            name: "wait_background".to_string(),
            description: "Wait for a backgrounded command (started by run_bash) to finish and return its output. Waits indefinitely by default.".to_string(),
            parameters: json!({"type": "object", "required": ["pid"],
                "properties": {"pid": {"type": "integer"},
                "log_path": {"type": "string"},
                "timeout": {"type": "integer", "description": "Max seconds to wait (default 0 = wait indefinitely)"}}}),
        },
    }
}

fn all_tools() -> Vec<ToolDefinition> {
    vec![
        read_file_tool(),
        list_dir_tool(),
        write_file_tool(),
        edit_file_tool(),
        run_bash_tool(),
        wait_background_tool(),
    ]
}

/// System prompt, seeded once per session (minion.py:1894 SYSTEM, trimmed to
/// our 3 tools). Without this the model has no idea it's a coding agent, what
/// the tools are for, or that `read.rs` means `src/read.rs` — so it guesses
/// list_dir(".") on everything.
fn system_prompt() -> Message {
    Message::system(
        "You are a terminal coding agent working in the user's current directory. \
        Use the provided tools to inspect and modify code. Take one concrete step at a time.\n\
        \n\
        Tool routing (pick exactly one):\n\
        - User names a FILE (e.g. \"look into read.rs\", \"open Cargo.toml\") -> call read_file with its path. \
        Try the path as given first (\"src/read.rs\" and \"read.rs\" are both worth trying; bare names usually live under src/).\n\
        - User names a FOLDER or says \"list / explore / what's in X\" -> call list_dir on it.\n\
        - User says \"all files\" with no folder -> call list_dir on \".\" first, then descend into subfolders (names ending in '/') and read_file files you find.\n\
        \n\
        If the runtime does NOT support native tool calls, emit a standalone text-protocol call exactly like:
        [minion_tool_call]{\"name\": \"read_file\", \"arguments\": {\"path\": \"foo.py\"}}[/minion_tool_call]
        Emit nothing before or after a tool call; wait for the Observation. When the task is done, reply in plain prose.",
    )
}

/// P2 agent-loop cap (minion.py:3814 MAX_MODEL_TURNS=200).
const MAX_MODEL_TURNS: u32 = 200;
/// P2 context discipline seed (minion.py:2145 TOOL_RESULT_CHARS=20000).
const TOOL_RESULT_CHARS: usize = 20_000;

/// Collapse runs of >=3 identical consecutive lines, then head/tail cap.
/// Minimal port of minion.py:_sanitize_tool_result (delimiter escaping and
/// full protocol handling land with the text-protocol fallback in P7).
fn sanitize_tool_result(result: &str) -> String {
    // 1. collapse >=3 identical consecutive lines to 2 + marker
    let lines: Vec<&str> = result.lines().collect();
    let mut collapsed: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        let run = j - i;
        if run >= 3 {
            collapsed.push(lines[i].to_string());
            collapsed.push(lines[i].to_string());
            collapsed.push(format!("... [+{} identical lines elided]", run - 1));
        } else {
            for line in lines.iter().take(j).skip(i) {
                collapsed.push(line.to_string());
            }
        }
        i = j;
    }
    let mut joined = collapsed.join("\n");
    if result.ends_with('\n') {
        joined.push('\n');
    }
    // minion: escape protocol delimiters first, then short-circuit small results
    let mut out = escape_tool_protocol_delimiters(&joined);
    if out.len() <= 1000 {
        return out;
    }
    // 2. head/tail cap with visible elision marker
    if out.len() > TOOL_RESULT_CHARS {
        let head = TOOL_RESULT_CHARS * 2 / 3;
        let tail = TOOL_RESULT_CHARS - head;
        // stay on char boundaries
        let mut h = head;
        while h > 0 && !out.is_char_boundary(h) {
            h -= 1;
        }
        let mut t = out.len() - tail;
        while t < out.len() && !out.is_char_boundary(t) {
            t += 1;
        }
        let elided = out.len() - (h + (out.len() - t));
        out = format!(
            "{}
... [{} chars elided to bound context; re-run more narrowly if you need the rest]
{}",
            &out[..h],
            elided,
            &out[t..]
        );
    }
    out
}

/// Port of minion.py:parse_text_calls (line 2636) + protocol escaping (2666).
/// Only standalone messages count: the whole trimmed text must be one or
/// more [minion_tool_call]{...}[/minion_tool_call] blocks (legacy
/// <tool_call> form also accepted). Anything with prose around it is NOT
/// a call -- this stops code examples from executing.
fn parse_text_calls(content: &str) -> Vec<(String, Value)> {
    let mut calls = vec![];
    let mut rest = content.trim();
    if rest.is_empty() {
        return calls;
    }
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let (open, close) = if rest.starts_with("[minion_tool_call]") {
            ("[minion_tool_call]", "[/minion_tool_call]")
        } else if rest.starts_with("<tool_call>") {
            ("<tool_call>", "</tool_call>")
        } else {
            return vec![];
        };
        let body = &rest[open.len()..];
        let end = match body.find(close) {
            Some(i) => i,
            None => return vec![],
        };
        let obj: Value = match serde_json::from_str(body[..end].trim()) {
            Ok(v) => v,
            Err(_) => return vec![],
        };
        let name = match obj.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => return vec![],
        };
        let args = obj.get("arguments").cloned().unwrap_or(json!({}));
        calls.push((name, args));
        rest = body[end + close.len()..].trim();
    }
    calls
}

/// Port of minion.py:_escape_tool_protocol_delimiters (line 2666):
/// neutralize active protocol tags in untrusted tool output so a file
/// containing [minion_tool_call] can never hijack the next turn.
fn escape_tool_protocol_delimiters(text: &str) -> String {
    let mut safe = text
        .replace("[minion_tool_call]", "&#91;minion_tool_call&#93;")
        .replace("[/minion_tool_call]", "&#91;/minion_tool_call&#93;");
    if safe.contains("<tool_call") || safe.contains("</tool_call") {
        safe = safe
            .replace("<tool_call", "&lt;tool_call")
            .replace("</tool_call", "&lt;/tool_call");
    }
    if safe != text {
        format!("[minion note: escaped tool-call protocol delimiters from this tool result before sending it back to the model]
{safe}")
    } else {
        safe
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostMessage {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

/// One chat message. Extra fields only appear where Ollama expects them:
/// - assistant messages may carry `tool_calls`
/// - tool messages carry `tool_name`
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

impl Message {
    fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
            tool_calls: None,
            tool_name: None,
        }
    }
    fn system(content: &str) -> Self {
        Self {
            role: "system".to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_name: None,
        }
    }
    fn assistant(content: String, tool_calls: Option<Vec<Value>>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls,
            tool_name: None,
        }
    }
    fn tool(name: String, content: String) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_calls: None,
            tool_name: Some(name),
        }
    }
}

/// Send one streaming request, print content live,
/// and return (full_text, tool_calls). Tool calls arrive as
/// message.tool_calls = [{function: {name, arguments: {...}}}] (arguments is an OBJECT here).
async fn stream_once(
    client: &reqwest::Client,
    post_url: &str,
    model: &str,
    history: &[Message],
    tools: Option<Vec<ToolDefinition>>,
) -> Result<(String, Vec<Value>), Box<dyn std::error::Error>> {
    let body = PostMessage {
        model: model.to_string(),
        messages: history.to_vec(),
        stream: true,
        tools,
    };
    let response = client.post(post_url).json(&body).send().await?;
    if !response.status().is_success() {
        println!("API request failed with status: {}", response.status());
        return Ok((String::new(), vec![]));
    }
    let mut stream = response.bytes_stream();
    let mut full_text = String::new();
    let mut tool_calls: Vec<Value> = vec![];
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line)?;
            if let Some(t) = v["message"]["content"].as_str() {
                print!("{t}");
                io::stdout().flush()?;
                full_text.push_str(t);
            }
            // Ollama may send tool_calls on any chunk; last one wins, so replace.
            if let Some(calls) = v["message"]["tool_calls"].as_array() {
                tool_calls = calls.clone();
            }
            if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
                break;
            }
        }
    }
    println!();
    Ok((full_text, tool_calls))
}

pub async fn talk_to_model(
    base_url: &str,
    model: &str,
    prompt: String,
    history: &mut Vec<Message>,
) -> Result<(), Box<dyn std::error::Error>> {
    let post_url = format!("{base_url}/api/chat");
    let model = model.to_string();
    let client = reqwest::Client::new();
    let tools = Some(all_tools());

    // each model turn either answers (Done), requests tools (Tool -> run,
    // push results, loop again), or comes back empty (auto-continue, capped).
    // Malformed/empty counters reset on every successful tool turn; the
    // MAX_MODEL_TURNS cap logs instead of silently stopping.
    // Seed the system prompt once per session (minion.py:3897 comment).
    // It rides along on every turn and tells the model what the tools do.
    if history.is_empty() || history[0].role != "system" {
        history.insert(0, system_prompt());
    }
    history.push(Message::user(prompt));
    let mut empty_turns: u32 = 0;
    for _turn in 1..=MAX_MODEL_TURNS {
        let (text, calls) = stream_once(&client, &post_url, &model, history, tools.clone()).await?;
        // P4 text-protocol fallback (minion.py:2636): small models that miss
        // native tool_calls can still emit [minion_tool_call]{...}[/minion_tool_call].
        // Only standalone blocks count -- prose around them means plain answer.
        let mut calls = calls;
        let mut text = text;
        if calls.is_empty() {
            let proto = parse_text_calls(&text);
            if !proto.is_empty() {
                calls = proto
                    .iter()
                    .map(|(name, args)| json!({"function": {"name": name, "arguments": args}}))
                    .collect();
                text = String::new();
            }
        }
        let has_calls = !calls.is_empty();
        let text_empty = text.trim().is_empty();
        history.push(Message::assistant(text, has_calls.then_some(calls.clone())));

        if has_calls {
            empty_turns = 0; // TURN_TOOL resets the empty counter (minion.py:3816)

            // Run each requested tool locally (Minion's DISPATCH, minion.py:1854).
            // Unknown tools and errors become tool messages too, so the model
            // sees what happened instead of hanging on a missing observation.
            for call in &calls {
                let name = call["function"]["name"].as_str().unwrap_or("?");
                let args = &call["function"]["arguments"];
                let args = if args.is_object() {
                    args.clone()
                } else {
                    json!({})
                };
                match parse_tool(name, &args) {
                    Ok(tool) => {
                        // Cyan tool box (minimal P2 ui): name + compact args
                        // preview <=120 chars, result preview <=800 chars.
                        let arg_str = serde_json::to_string(&args).unwrap_or_default();
                        let mut arg_prev: String = arg_str.chars().take(120).collect();
                        if arg_str.len() > 120 {
                            arg_prev.push_str("...");
                        }
                        println!("\x1b[36m[tool {name} {arg_prev}]\x1b[0m");
                        let raw = tool.run().unwrap_or_else(|e| format!("ERROR: {e}"));
                        let out = sanitize_tool_result(&raw);
                        let preview: String = out.chars().take(800).collect();
                        println!("\x1b[36m[result]\x1b[0m\n{preview}");
                        history.push(Message::tool(name.to_string(), out));
                    }
                    Err(e) => {
                        println!("\x1b[33m[unknown tool {name}: {e}]\x1b[0m");
                        history.push(Message::tool(
                            name.to_string(),
                            format!("Unknown tool: {e}"),
                        ));
                    }
                }
            }
            continue; // loop again with the tool observations in context
        }

        if !text_empty {
            return Ok(()); // TURN_DONE: plain answer, nothing more to do
        }

        // TURN_EMPTY: no content, no tool calls — auto-continue, capped.
        empty_turns += 1;
        if empty_turns >= 3 {
            println!("[empty turn x{empty_turns}: giving up, returning control]");
            return Ok(());
        }
        println!("[empty turn x{empty_turns}: continuing]");
    }
    println!("[max turns ({MAX_MODEL_TURNS}) reached: stopping, returning control]");
    Ok(())
}

#[cfg(test)]
mod proto_tests {
    use super::*;

    #[test]
    fn standalone_bracket_block_parses() {
        let c = parse_text_calls(
            r#"[minion_tool_call]{"name": "read_file", "arguments": {"path": "x"}}[/minion_tool_call]"#,
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].0, "read_file");
        assert_eq!(c[0].1["path"], json!("x"));
    }

    #[test]
    fn prose_around_block_is_not_a_call() {
        let c = parse_text_calls(
            "here is how: [minion_tool_call]{\"name\": \"read_file\", \"arguments\": {}}[/minion_tool_call]",
        );
        assert!(c.is_empty());
        let c = parse_text_calls("[minion_tool_call]{not json}[/minion_tool_call]");
        assert!(c.is_empty());
    }

    #[test]
    fn tool_output_delimiters_get_escaped() {
        let out = escape_tool_protocol_delimiters("[minion_tool_call]{}");
        assert!(out.contains("&#91;minion_tool_call&#93;"));
        assert!(out.contains("minion note"));
        let clean = escape_tool_protocol_delimiters("plain output");
        assert_eq!(clean, "plain output");
    }

    #[test]
    fn sanitize_matches_minion_markers() {
        // collapse style: keep 1 + elide marker with run-1 count
        let rep = "same\n".repeat(5);
        let s = sanitize_tool_result(&rep);
        assert!(s.contains("[+4 identical lines elided]"), "{s}");
        // short results pass through
        assert_eq!(sanitize_tool_result("tiny"), "tiny");
        // long results get the minion elision marker
        let big = "x".repeat(25_000);
        let s = sanitize_tool_result(&big);
        assert!(
            s.contains("chars elided to bound context"),
            "{}",
            &s[s.len() - 200..]
        );
    }
}
