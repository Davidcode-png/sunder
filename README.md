# Sunder

A lightweight terminal coding agent in Rust. Talk to a local model, let it read, write, edit, and run commands in your project.

Inspired by [minion](https://github.com/Sentdex/minion) — one file, one idea: keep the token floor low so local models stay fast and smart. No TUI framework, no plugin system. Just a REPL, six tools, and raw tool calls over HTTP. A vendored copy of minion lives in `reference/` with the full roadmap in `ROADMAP.txt`.

## Setup

You need Rust (stable) and a model server. Ollama works out of the box:

```sh
ollama pull qwen2.5:3b
ollama serve
```

Then:

```sh
cargo run
```

Useful flags (or env vars):

```sh
cargo run -- --model qwen2.5:3b --url http://localhost:11434
cargo run -- --yolo          # skip write/run confirmations (or SUNDER_YOLO=1)
```

Type `exit` to quit. `read_file` pages large files (400 lines by default), and `run_bash` backgrounds slow commands — check them later with `wait_background` or by reading the log.
