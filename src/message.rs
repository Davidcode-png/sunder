use clap::Parser;
use futures_util::StreamExt;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::Value;


#[derive(Debug, Parser)]
pub struct PromptArgs {
    pub chat: String,
}

#[derive(Debug, Serialize, Clone)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize, Clone)]
struct ToolDefinition {
    r#type: String,
    function: FunctionDefinition,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostMessage {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    // tools: Vec<ToolDefinition>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ToolCall {
    function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct FunctionCall {
    name: String,
    arguments: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    role: String,
    content: String,
}


pub async fn talk_to_model(
    base_url: &str,
    prompt: String,
    history: &mut Vec<Message>,
) -> Result<(), Box<dyn std::error::Error>> {
    let post_url = format!("{base_url}/api/chat");
    history.push(Message {
        role: "user".to_string(),
        content: prompt,
    });
    let new_mesesage = PostMessage {
        model: "qwen2.5:3b".to_string(),
        messages: history.clone(),
        stream: true,
    };
    let client = reqwest::Client::new();
    let response = client.post(post_url).json(&new_mesesage).send().await?;
    if !response.status().is_success() {
        println!("API request failed with status: {}", response.status());

        return Ok(());
    }
    let mut stream = response.bytes_stream();
    let mut full_response = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let text = String::from_utf8_lossy(&chunk);
        // println!("Just seeing this out {}", text);
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let response_body: Value = serde_json::from_str(line)?;
            if let Some(text) = response_body["message"]["content"].as_str() {
                print!("{}", text);
                use std::io::{self, Write};
                io::stdout().flush()?;

                full_response.push_str(text);
            }
        }
    }
    println!();

    history.push(Message {
        role: "assistant".to_string(),
        content: full_response,
    });
    Ok(())
}
