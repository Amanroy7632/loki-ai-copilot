//! `loki models …`: recommend, list, pull (with license notice, progress, cancel), use.

use std::io::{BufRead, Write};

use anyhow::{Context as _, bail};
use clap::Subcommand;
use futures::StreamExt as _;
use loki_runtime::models;
use loki_runtime::{CancellationToken, Runtime};

#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Detect this machine and suggest a verified model.
    Recommend,
    /// Models installed in Ollama.
    List,
    /// Download an allowlisted model through Ollama. Shows the license first; Ctrl+C cancels.
    Pull {
        /// Model ID, e.g. `qwen3:4b` (see `loki models recommend`).
        id: String,
        /// Accept the license without the interactive prompt (scripts, CI).
        #[arg(long)]
        accept_license: bool,
    },
    /// Save a model (and the Ollama provider) as the default in the config.
    Use {
        /// Model ID, e.g. `qwen3:4b`.
        id: String,
    },
}

const GB: f64 = 1_000_000_000.0;

pub async fn run(runtime: &mut Runtime, command: ModelsCommand) -> anyhow::Result<()> {
    match command {
        ModelsCommand::Recommend => recommend(runtime),
        ModelsCommand::List => list(runtime).await,
        ModelsCommand::Pull { id, accept_license } => pull(runtime, &id, accept_license).await,
        ModelsCommand::Use { id } => {
            runtime.use_model(&id)?;
            println!("Saved: provider ollama, model {id}. Start chatting with `loki chat`.");
            Ok(())
        }
    }
}

fn recommend(runtime: &Runtime) -> anyhow::Result<()> {
    let (profile, recommendation) = runtime.recommend_models();
    println!("Machine: {} on {}", profile.arch, profile.os);
    println!(
        "RAM:     {} GB total, {:.1} GB available",
        profile.ram_gb(),
        profile.available_ram as f64 / GB
    );
    match profile.free_disk {
        Some(free) => println!("Disk:    {:.1} GB free", free as f64 / GB),
        None => println!("Disk:    unknown"),
    }
    println!("GPU:     {}", profile.gpu);
    println!();
    println!("{}", recommendation.reason);
    if let Some(primary) = &recommendation.primary {
        println!(
            "\nRecommended: {}  ({}, {}, ~{:.1} GB download)",
            primary.id,
            primary.display_name,
            primary.license,
            primary.download_bytes as f64 / GB
        );
        println!("  {}", primary.notes);
        println!(
            "  Next: loki models pull {}   then   loki models use {}",
            primary.id, primary.id
        );
    }
    for alternative in &recommendation.alternatives {
        println!(
            "Also verified: {}  ({}, ~{:.1} GB): {}",
            alternative.id,
            alternative.license,
            alternative.download_bytes as f64 / GB,
            alternative.notes
        );
    }
    for unverified in &recommendation.unverified {
        println!(
            "Not yet verified (not recommended): {}  {}",
            unverified.id, unverified.notes
        );
    }
    Ok(())
}

async fn list(runtime: &Runtime) -> anyhow::Result<()> {
    let installed = runtime.installed_models().await?;
    if installed.is_empty() {
        println!("No models installed. See `loki models recommend`.");
    }
    for model in installed {
        let reviewed = if models::find(&model.name).is_some() {
            "reviewed"
        } else {
            "not in Loki's reviewed list"
        };
        println!(
            "{:<24} {:>6.1} GB  {reviewed}",
            model.name,
            model.size as f64 / GB
        );
    }
    Ok(())
}

fn confirm_license(entry: &models::ModelEntry) -> anyhow::Result<bool> {
    println!(
        "{} ({}) is licensed under {}.",
        entry.display_name, entry.id, entry.license
    );
    println!("License text: {}", entry.license_url);
    println!(
        "Download size: about {:.1} GB. Ollama downloads it and verifies each layer's sha256.",
        entry.download_bytes as f64 / GB
    );
    print!("Accept the license and download? Type 'yes': ");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut answer)
        .context("could not read the answer")?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

async fn pull(runtime: &Runtime, id: &str, accept_license: bool) -> anyhow::Result<()> {
    let Some(entry) = models::find(id) else {
        bail!("'{id}' is not in Loki's reviewed model list. See `loki models recommend`.");
    };
    if !accept_license && !confirm_license(&entry)? {
        println!("Cancelled; nothing downloaded.");
        return Ok(());
    }
    let cancel = CancellationToken::new();
    let mut progress = runtime.pull_model(id, cancel.clone())?;
    let mut last_status = String::new();
    loop {
        let next = tokio::select! {
            next = progress.next() => next,
            _ = crate::signals::ctrl_c() => {
                cancel.cancel();
                continue;
            }
        };
        match next {
            None => break,
            Some(Ok(update)) => {
                match (update.completed, update.total) {
                    (Some(done), Some(total)) if total > 0 => {
                        print!(
                            "\r{:<40} {:>5.1}% of {:.2} GB",
                            short(&update.status),
                            done as f64 * 100.0 / total as f64,
                            total as f64 / GB
                        );
                    }
                    _ if update.status != last_status => print!("\n{}", update.status),
                    _ => {}
                }
                std::io::stdout().flush().ok();
                last_status = update.status;
            }
            Some(Err(error)) if error.is_cancelled() => {
                println!("\nCancelled. Run the same pull again to continue.");
                return Ok(());
            }
            Some(Err(error)) => {
                println!();
                return Err(error).context("pull failed");
            }
        }
    }
    println!("\nDone. Select it with `loki models use {id}`.");
    Ok(())
}

fn short(status: &str) -> String {
    status.chars().take(40).collect()
}
