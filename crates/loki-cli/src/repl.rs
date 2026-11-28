//! The `loki chat` read-eval-print loop.
//!
//! Stopping a reply: Ctrl+C, or type `/stop` and Enter while it streams. Once Tokio's `ctrl_c`
//! handler is installed, SIGINT (Ctrl+C on Windows) no longer kills the process. At the prompt,
//! Ctrl+C prints a hint; `/quit` or end of input (Ctrl+D) exits.
//!
//! Lines typed while a reply streams (other than `/stop`) are queued and handled afterwards,
//! so piped input (tests, scripts) is never lost.

use std::collections::VecDeque;
use std::io::{BufRead, Write};

use futures::future::BoxFuture;
use loki_core::ConversationId;
use loki_runtime::{
    CancellationToken, PendingApproval, ProviderHealth, ReplyOutcome, Runtime, ToolEvent, ToolUi,
};
use tokio::sync::{mpsc, oneshot};

/// Terminal approvals: shows the full preview and waits for a line typed **after** the prompt.
/// Only `y` or `yes` approves; anything else (including an empty line) is a no.
#[derive(Default)]
struct CliUi {
    /// `closed` and the waiting sender share one lock, so `close()` can never slip in between
    /// "is stdin open?" and "register me as waiting" (which would leave an approval hanging).
    state: std::sync::Mutex<ApprovalState>,
}

#[derive(Default)]
struct ApprovalState {
    closed: bool,
    waiting: Option<oneshot::Sender<String>>,
}

impl CliUi {
    /// No more input will come: a waiting approval and every future one are a no.
    fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            if let Some(sender) = state.waiting.take() {
                let _ = sender.send(String::new());
            }
        }
    }

    /// Delivers `line` to a waiting approval. Returns `false` if nothing was waiting.
    fn answer(&self, line: &str) -> bool {
        let sender = self.state.lock().ok().and_then(|mut s| s.waiting.take());
        match sender {
            Some(sender) => {
                let _ = sender.send(line.to_owned());
                true
            }
            None => false,
        }
    }
}

impl ToolUi for CliUi {
    fn approve<'a>(&'a self, pending: &'a PendingApproval) -> BoxFuture<'a, bool> {
        println!(
            "\n\nLoki wants to run a tool that needs your approval:\n{}",
            pending.preview()
        );
        let (sender, receiver) = oneshot::channel();
        let registered = match self.state.lock() {
            Ok(mut state) if !state.closed => {
                state.waiting = Some(sender);
                true
            }
            _ => false,
        };
        if !registered {
            println!("No input is available to answer, so it is not approved.");
            return Box::pin(async { false });
        }
        print!("Approve? [y/N] ");
        flush();
        Box::pin(async move {
            let answer = receiver.await.unwrap_or_default();
            let yes = matches!(answer.trim().to_lowercase().as_str(), "y" | "yes");
            println!("{}", if yes { "Approved." } else { "Not approved." });
            yes
        })
    }

    fn notify(&self, event: &ToolEvent) {
        let ToolEvent::Finished {
            tool,
            status,
            detail,
        } = event;
        match detail {
            Some(detail) => println!("\n[tool {tool}: {} ({detail})]", status.as_str()),
            None => println!("\n[tool {tool}: {}]", status.as_str()),
        }
    }
}

/// What the user typed at the prompt.
enum Input {
    Quit,
    Retry,
    Stop,
    Help,
    Model(Option<String>),
    Empty,
    Text(String),
}

fn parse(line: &str) -> Input {
    let trimmed = line.trim();
    match trimmed {
        "/quit" | "/exit" => Input::Quit,
        "/retry" => Input::Retry,
        "/stop" => Input::Stop,
        "/help" => Input::Help,
        "/model" => Input::Model(None),
        "" => Input::Empty,
        _ => match trimmed.strip_prefix("/model ") {
            Some(name) => Input::Model(Some(name.trim().to_owned())),
            None => Input::Text(trimmed.to_owned()),
        },
    }
}

/// Reads stdin on a dedicated thread (stdin reads block) and forwards lines to the async loop.
fn spawn_stdin_reader() -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel(64);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.blocking_send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

fn flush() {
    let _ = std::io::stdout().flush();
}

fn label(runtime: &Runtime) -> String {
    match runtime.model() {
        Some(model) => format!("{} {model}", runtime.provider_name()),
        None => runtime.provider_name().to_owned(),
    }
}

/// Input source with a queue for lines that arrived while a reply was streaming.
struct Lines {
    receiver: mpsc::Receiver<String>,
    queued: VecDeque<String>,
    closed: bool,
}

impl Lines {
    async fn next(&mut self) -> Option<String> {
        match self.queued.pop_front() {
            Some(line) => Some(line),
            None => self.receiver.recv().await,
        }
    }
}

/// Runs the REPL until `/quit` or end of input.
pub async fn run(runtime: &mut Runtime, resume: Option<ConversationId>) -> anyhow::Result<()> {
    let mut conversation = match resume {
        Some(id) => {
            let existing = runtime.conversation(&id).await?;
            println!("Continuing \"{}\" ({})", existing.title, existing.id);
            Some(existing.id)
        }
        None => None,
    };
    println!(
        "Loki chat. Provider: {}. Type /help for commands.",
        label(runtime)
    );
    report_health(runtime).await;

    let mut lines = Lines {
        receiver: spawn_stdin_reader(),
        queued: VecDeque::new(),
        closed: false,
    };
    loop {
        if lines.queued.is_empty() {
            print!("you> ");
            flush();
        }
        let line = tokio::select! {
            line = lines.next() => line,
            _ = crate::signals::ctrl_c() => {
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
            Input::Stop => println!("Nothing is generating."),
            Input::Help => println!(
                "/stop          stop the current reply (or Ctrl+C)\n\
                 /retry         regenerate the last reply\n\
                 /model         show the model; /model <id> switches for this session\n\
                 /quit          exit"
            ),
            Input::Model(None) => println!("Provider: {}", label(runtime)),
            Input::Model(Some(name)) => match runtime.switch_model(&name) {
                Ok(()) => {
                    println!(
                        "Switched to {} (this session only; `loki models use {name}` saves it).",
                        label(runtime)
                    );
                    report_health(runtime).await;
                }
                Err(error) => println!("Can't switch: {error}"),
            },
            Input::Retry => match &conversation {
                Some(id) => reply(runtime, id, None, &mut lines).await?,
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
                reply(runtime, &id, Some(&text), &mut lines).await?;
            }
        }
    }
    if let Some(id) = conversation {
        println!("Saved as conversation {id}");
    }
    Ok(())
}

async fn report_health(runtime: &Runtime) {
    match runtime.provider_health().await {
        ProviderHealth::Ready => println!("Status: ready."),
        ProviderHealth::Unavailable { reason } => println!("Status: NOT READY: {reason}"),
    }
}

/// Streams one reply (`text` = new user message, `None` = retry), stoppable by Ctrl+C or `/stop`.
async fn reply(
    runtime: &Runtime,
    id: &ConversationId,
    text: Option<&str>,
    lines: &mut Lines,
) -> anyhow::Result<()> {
    print!("loki ({})> ", label(runtime));
    flush();
    let cancel = CancellationToken::new();
    let print_token = |token: &str| {
        print!("{token}");
        flush();
    };
    let ui = CliUi::default();
    if lines.closed {
        ui.close();
    }
    let generation = async {
        match text {
            Some(text) => {
                runtime
                    .send(id, text, cancel.clone(), &ui, print_token)
                    .await
            }
            None => {
                runtime
                    .retry_reply(id, cancel.clone(), &ui, print_token)
                    .await
            }
        }
    };
    tokio::pin!(generation);
    let mut stdin_open = !lines.closed;
    let outcome = loop {
        tokio::select! {
            outcome = &mut generation => break outcome,
            _ = crate::signals::ctrl_c() => cancel.cancel(),
            line = lines.receiver.recv(), if stdin_open => match line {
                // A pending approval takes the next line typed after its prompt.
                Some(line) if ui.answer(&line) => {}
                Some(line) if line.trim() == "/stop" => cancel.cancel(),
                Some(line) => lines.queued.push_back(line),
                None => {
                    // stdin closed: no one can approve, so any pending approval is a no.
                    stdin_open = false;
                    lines.closed = true;
                    ui.close();
                }
            },
        }
    };

    match outcome {
        Ok(ReplyOutcome::Completed(_)) => println!(),
        Ok(ReplyOutcome::Stopped(Some(_))) => {
            println!("\n[stopped; partial reply saved. /retry to regenerate]")
        }
        Ok(ReplyOutcome::Stopped(None)) => {
            println!("\n[stopped; nothing saved. /retry to regenerate]")
        }
        // A failed reply isn't fatal to the session: report it and keep the REPL alive.
        // The user's message is already stored, so /retry works once the cause is fixed.
        Err(error) => println!("\n[error: {error}. Fix it, then /retry]"),
    }
    Ok(())
}
