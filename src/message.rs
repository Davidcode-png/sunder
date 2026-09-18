use reqwest;
use reqwest::Error;
use serde::{Deserialize, Serialize};
use clap::Parser;
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

pub async fn talk_to_model(base_url: &str, prompt: String, history: &mut Vec<String>) -> Result<(), Error> {
    let post_url = format!("{base_url}/api/generate");

    let value = prompt + &history.join("\n");

    let new_mesesage = PostMessage {
        model: "qwen2.5:3b".to_string(),
        prompt: value,
        stream: false,
    };
    let client = reqwest::Client::new();
    let response = client.post(post_url).json(&new_mesesage).send().await?;
    if response.status().is_success() {
        let response_body: Value = response.json().await?;
        if let Some(text) = response_body["response"].as_str(){
            println!("{text}");
            history.push(text.to_string());
        }
    } else {
        println!("API request failed with status: {}", response.status());
    }
    Ok(())
}
