use Sunder::message::{PromptArgs, talk_to_model};
use clap::Parser;

#[tokio::main]
async fn main() {
    let args = PromptArgs::parse();
    let _ = talk_to_model("http://localhost:11434", args.chat).await;

}