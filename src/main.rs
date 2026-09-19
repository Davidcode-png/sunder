use std::io::{self, Write};

use Sunder::message::{PromptArgs, talk_to_model};
use clap::Parser;

#[tokio::main]
async fn main() {
    let mut history: Vec<String> = vec![];
    loop {
        print!("> ");
        io::stdout().flush();

        let mut input = String::new();
        let _ = io::stdin().read_line(&mut input);

        let input = input.trim();

        if input == "exit" {
            break;
        }
        let _ = talk_to_model("http://localhost:11434", input.to_string(), &mut history).await;
        // println!();
    }
}
