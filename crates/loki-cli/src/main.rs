//! `loki`: the headless development harness.
//!
//! This lets every layer (provider, memory, tools, policy, voice) be exercised without the
//! desktop UI. It is also how CI and coding agents run end-to-end checks.
//!
//! Commands: `loki chat [--conversation <id>]`, `loki history list|show <id>`,
//! `loki models recommend|list|pull <id>|use <id>`. Global: `--data-dir`, `--provider`, `--model`.
//! All behaviour lives in `loki-runtime`; this crate only parses input and prints output.

mod models;
mod repl;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use loki_core::ConversationId;
use loki_runtime::{ProviderKind, Runtime, RuntimeOptions};

/// Delay between mock tokens, so streaming and Ctrl+C are observable with `--provider mock`.
const MOCK_TOKEN_DELAY: Duration = Duration::from_millis(40);

#[derive(Debug, Parser)]
#[command(
    name = "loki",
    version,
    about = "Loki headless harness: local chat over Ollama (or a mock)"
)]
struct Cli {
    /// Use this directory for data, config and logs instead of the per-user default.
    /// `LOKI_DATA_DIR` does the same; this flag wins.
    #[arg(long, global = true, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Model backend for this run (default: from config, `ollama` for new installs).
    #[arg(long, global = true, value_enum)]
    provider: Option<ProviderArg>,

    /// Model for this run, e.g. `qwen3:4b` (default: from config).
    #[arg(long, global = true, value_name = "ID")]
    model: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProviderArg {
    /// Local Ollama server (loopback only).
    Ollama,
    /// Scripted echo replies, for tests and demos. Replies are labelled as mock.
    Mock,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Chat in a REPL. Ctrl+C or /stop stops a reply; /retry regenerates it; /quit exits.
    Chat {
        /// Continue an existing conversation instead of starting a new one.
        #[arg(long, value_name = "ID")]
        conversation: Option<String>,
    },
    /// Inspect stored conversations.
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
    /// Choose, download and select local models (Ollama).
    Models {
        #[command(subcommand)]
        command: models::ModelsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum HistoryCommand {
    /// List conversations, most recent first: `<id>  <title>`.
    List,
    /// Print every message of one conversation.
    Show {
        /// The conversation ID (from `history list`).
        id: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    // `history` and `models` never talk to the chat provider, so they must work even when no
    // model is chosen yet: open them with the mock provider.
    let chat = matches!(cli.command, Command::Chat { .. });
    let provider = match cli.provider {
        Some(ProviderArg::Ollama) => Some(ProviderKind::Ollama),
        Some(ProviderArg::Mock) => Some(ProviderKind::Mock),
        None if chat => None,
        None => Some(ProviderKind::Mock),
    };
    // Opening is blocking (files, SQLite), so it happens before the async runtime starts.
    let mut runtime = Runtime::open(RuntimeOptions {
        data_dir: cli.data_dir,
        provider,
        model: cli.model,
        mock_token_delay: MOCK_TOKEN_DELAY,
        init_logging: true,
    })
    .context("could not start Loki")?;
    for warning in runtime.startup_warnings() {
        eprintln!("warning: {warning}");
    }

    let executor = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not start the async runtime")?;
    executor.block_on(async {
        match cli.command {
            Command::Chat { conversation } => {
                repl::run(&mut runtime, conversation.map(ConversationId::new)).await
            }
            Command::History {
                command: HistoryCommand::List,
            } => history_list(&runtime).await,
            Command::History {
                command: HistoryCommand::Show { id },
            } => history_show(&runtime, &ConversationId::new(id)).await,
            Command::Models { command } => models::run(&mut runtime, command).await,
        }
    })
}

async fn history_list(runtime: &Runtime) -> anyhow::Result<()> {
    let conversations = runtime.list_conversations().await?;
    if conversations.is_empty() {
        println!("No conversations yet. Start one with `loki chat`.");
    }
    for conversation in conversations {
        println!("{}  {}", conversation.id, conversation.title);
    }
    Ok(())
}

async fn history_show(runtime: &Runtime, id: &ConversationId) -> anyhow::Result<()> {
    let conversation = runtime.conversation(id).await?;
    println!("# {} ({})", conversation.title, conversation.id);
    for message in runtime.messages(id).await? {
        let stopped = if message.stopped { " [stopped]" } else { "" };
        println!("[{}]{stopped} {}", message.role.as_str(), message.content);
    }
    Ok(())
}
