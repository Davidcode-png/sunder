use std::io::{self, Write};

use clap::Parser;
use sunder::message::{Message, talk_to_model};

/// --model (SUNDER_MODEL, default qwen2.5:3b), --url (SUNDER_URL,
/// default http://localhost:11434), --yolo (SUNDER_YOLO=1 skips confirm).
#[derive(Parser, Debug)]
struct Args {
    #[arg(long, env = "SUNDER_MODEL", default_value = "qwen2.5:3b")]
    model: String,
    #[arg(long, env = "SUNDER_URL", default_value = "http://localhost:11434")]
    url: String,
    #[arg(long, env = "SUNDER_YOLO", default_value_t = false)]
    yolo: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    if args.yolo {
        // SAFETY: single-threaded startup, no other writers yet.
        unsafe { std::env::set_var("SUNDER_YOLO", "1") };
    }
    eprintln!(
        "sunder | model={} url={}{}",
        args.model,
        args.url,
        if args.yolo { " yolo=1" } else { "" }
    );
    let mut history: Vec<Message> = vec![];
    loop {
        print!("> ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);

        let input = input.trim();

        if input == "exit" {
            break;
        }
        if input.is_empty() {
            continue;
        }
        if let Err(e) = talk_to_model(&args.url, &args.model, input.to_string(), &mut history).await
        {
            eprintln!("[error] {e}");
        }
    }
}
