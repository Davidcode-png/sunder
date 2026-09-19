use clap::Parser;
use futures_util::StreamExt;
use reqwest;
use reqwest::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub enum Message {}

#[derive(Debug, Parser)]
pub struct PromptArgs {
    pub chat: String,
}

#[derive(Debug, Serialize, Deserialize, Parser)]
pub struct PostMessage {
    model: String,
    prompt: String,
    stream: bool,
}

pub async fn talk_to_model(
    base_url: &str,
    prompt: String,
    history: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let post_url = format!("{base_url}/api/generate");

    let value = format!("{}\n{}", history.join("\n"), prompt);

    let new_mesesage = PostMessage {
        model: "qwen2.5:3b".to_string(),
        prompt: value,
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
            if let Some(text) = response_body["response"].as_str() {
                print!("{}", text);
                use std::io::{self, Write};
                io::stdout().flush()?;

                full_response.push_str(text);
            }
        }
    }
    println!();

    history.push(prompt);
    history.push(full_response);
    Ok(())
}
