//! The `loki chat` read-eval-print loop.
//!
//! Ctrl+C handling: once Tokio's `ctrl_c` handler is installed, SIGINT (or Ctrl+C on Windows) no
//! longer kills the process. During a reply it cancels generation; at the prompt it prints a
//! hint. `/quit` or end of input (Ctrl+D) exits.

use std::io::{BufRead, Write};

use anyhow::Context as _;
use loki_core::ConversationId;
use loki_runtime::{CancellationToken, ReplyOutcome, Runtime};
use tokio::sync::mpsc;

/// What the user typed at the prompt.
enum Input {
    Quit,
    Retry,
    Help,
    Empty,
    Text(String),
}

fn parse(line: &str) -> Input {
    let trimmed = line.trim();
    match trimmed {
        "/quit" | "/exit" => Input::Quit,
        "/retry" => Input::Retry,
        "/help" => Input::Help,
        "" => Input::Empty,
        _ => Input::Text(trimmed.to_owned()),
    }
}

/// Reads stdin on a dedicated thread (stdin reads block) and forwards lines to the async loop.
fn spawn_stdin_reader() -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel(16);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if sender.blocking_send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

fn prompt() {
    print!("you> ");
    let _ = std::io::stdout().flush();
}

/// Runs the REPL until `/quit` or end of input.
pub async fn run(runtime: &Runtime, resume: Option<ConversationId>) -> anyhow::Result<()> {
    let mut conversation = match resume {
        Some(id) => {
            let existing = runtime.conversation(&id).await?;
            println!("Continuing \"{}\" ({})", existing.title, existing.id);
            Some(existing.id)
        }
        None => None,
    };
    let label = runtime.provider_name().to_owned();
    println!("Loki chat. Provider: {label}. Type /help for commands.");

    let mut lines = spawn_stdin_reader();
    loop {
        prompt();
        let line = tokio::select! {
            line = lines.recv() => line,
            _ = tokio::signal::ctrl_c() => {
                println!("\n(Nothing to stop. Type /quit to exit.)");
                continue;
            }
        };
        let Some(line) = line else {
            println!();
            break;
        };
        match parse(&line) {
            Input::Quit => break,
            Input::Empty => {}
            Input::Help => {
                println!(
                    "/retry  regenerate the last reply\n/quit   exit\nCtrl+C  stop the current reply"
                );
            }
            Input::Retry => match &conversation {
                Some(id) => reply(runtime, id, &label, true).await?,
                None => println!("Nothing to retry yet."),
            },
            Input::Text(text) => {
                let id = match &conversation {
                    Some(id) => id.clone(),
                    None => {
                        let created = runtime.start_conversation(&text).await?;
                        conversation = Some(created.id.clone());
                        created.id
                    }
                };
                runtime.add_user_message(&id, &text).await?;
                reply(runtime, &id, &label, false).await?;
            }
        }
    }
    if let Some(id) = conversation {
        println!("Saved as conversation {id}");
    }
    Ok(())
}

/// Streams one reply, cancelling it on Ctrl+C.
async fn reply(
    runtime: &Runtime,
    id: &ConversationId,
    label: &str,
    retry: bool,
) -> anyhow::Result<()> {
    print!("loki ({label})> ");
    let _ = std::io::stdout().flush();
    let cancel = CancellationToken::new();
    let print_token = |token: &str| {
        print!("{token}");
        let _ = std::io::stdout().flush();
    };

    let generation = async {
        if retry {
            runtime.retry_reply(id, cancel.clone(), print_token).await
        } else {
            runtime
                .generate_reply(id, cancel.clone(), print_token)
                .await
        }
    };
    tokio::pin!(generation);
    let outcome = tokio::select! {
        outcome = &mut generation => outcome,
        _ = tokio::signal::ctrl_c() => {
            cancel.cancel();
            generation.await
        }
    }
    .context("reply failed")?;

    match outcome {
        ReplyOutcome::Completed(_) => println!(),
        ReplyOutcome::Cancelled { .. } => println!("\n[stopped; not saved. /retry to regenerate]"),
    }
    Ok(())
}
