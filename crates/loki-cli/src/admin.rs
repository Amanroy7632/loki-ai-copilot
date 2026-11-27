//! `loki permissions`, `loki roots`, `loki apps` and `loki audit` (P3-T09).
//! Output is redacted data only: audit rows hold summaries, never tool output or file contents.

use std::path::PathBuf;

use anyhow::bail;
use clap::Subcommand;
use loki_core::{CapabilityStatus, TaskId, UnixMillis};
use loki_runtime::{Runtime, ToolRunFilter};

#[derive(Debug, Subcommand)]
pub enum PermissionsCommand {
    /// Every capability: platform status and granted scopes.
    List,
    /// Allow tools to use a capability, e.g. `clock`, `system_info`, `app_launch`, `file_read`.
    Grant {
        /// Capability name.
        capability: String,
        /// `*` (all) or an allowed-root ID (for `file_read`).
        #[arg(long, default_value = "*")]
        scope: String,
    },
    /// Take a grant back.
    Revoke {
        /// Capability name.
        capability: String,
        /// The exact scope that was granted.
        #[arg(long, default_value = "*")]
        scope: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum RootsCommand {
    /// Folders the file tools may search and read.
    List,
    /// Allow a folder. The model only ever sees the ID, never the path.
    Add {
        /// Short ID: 1-32 of a-z, 0-9, _ or -.
        id: String,
        /// An existing directory.
        path: PathBuf,
    },
    /// Remove a folder.
    Remove {
        /// The root ID.
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Tool runs, newest first.
    List {
        /// Only this tool, e.g. `app.open`.
        #[arg(long)]
        tool: Option<String>,
        /// Only runs newer than this: `30m`, `2h`, `7d`, or Unix milliseconds.
        #[arg(long)]
        since: Option<String>,
        /// At most this many rows.
        #[arg(long, default_value_t = 50)]
        limit: u32,
    },
    /// One task: its state, the (redacted) request and its runs.
    Show {
        /// Task ID (from `audit list`).
        task_id: String,
    },
}

fn status_text(status: &CapabilityStatus) -> String {
    match status {
        CapabilityStatus::Supported => String::from("supported"),
        CapabilityStatus::PermissionRequired { reason } => format!("permission required: {reason}"),
        CapabilityStatus::Denied { reason } => format!("denied: {reason}"),
        CapabilityStatus::Unavailable { reason } => format!("unavailable: {reason}"),
    }
}

pub async fn permissions(runtime: &Runtime, command: PermissionsCommand) -> anyhow::Result<()> {
    match command {
        PermissionsCommand::List => {
            println!("{:<16} {:<12} status", "capability", "granted");
            for row in runtime.permissions().await? {
                let granted = if row.scopes.is_empty() {
                    String::from("-")
                } else {
                    row.scopes.join(",")
                };
                println!(
                    "{:<16} {:<12} {}",
                    row.capability.as_str(),
                    granted,
                    status_text(&row.status)
                );
            }
            if !runtime.tools_enabled() {
                println!("\nTools are disabled in config (tools.enabled = false).");
            }
        }
        PermissionsCommand::Grant { capability, scope } => {
            let grant = runtime.grant(&capability, &scope).await?;
            println!(
                "Granted {} (scope {}).",
                grant.capability.as_str(),
                grant.scope
            );
        }
        PermissionsCommand::Revoke { capability, scope } => {
            match runtime.revoke(&capability, &scope).await? {
                0 => println!("No active grant for {capability} (scope {scope})."),
                n => println!("Revoked {n} grant(s) for {capability} (scope {scope})."),
            }
        }
    }
    Ok(())
}

pub fn roots(runtime: &mut Runtime, command: RootsCommand) -> anyhow::Result<()> {
    match command {
        RootsCommand::List => {
            let roots = &runtime.config().allowed_roots;
            if roots.is_empty() {
                println!("No allowed folders. Add one with `loki roots add <id> <path>`.");
            }
            for root in roots {
                println!("{:<16} {}", root.id, root.path.display());
            }
        }
        RootsCommand::Add { id, path } => {
            runtime.add_root(&id, &path)?;
            println!(
                "Added root '{id}'. File tools also need: loki permissions grant file_read --scope {id}"
            );
        }
        RootsCommand::Remove { id } => {
            if runtime.remove_root(&id)? {
                println!("Removed root '{id}'.");
            } else {
                bail!("no root '{id}'");
            }
        }
    }
    Ok(())
}

pub async fn apps(runtime: &Runtime, query: Option<String>) -> anyhow::Result<()> {
    let query = query.unwrap_or_default().to_lowercase();
    for app in runtime.apps().await? {
        if query.is_empty()
            || app.display_name.to_lowercase().contains(&query)
            || app.app_id.to_lowercase().contains(&query)
        {
            println!("{:<32} {}", app.display_name, app.app_id);
        }
    }
    Ok(())
}

/// `30m`, `2h`, `7d` (relative to now) or a Unix-millisecond timestamp.
fn parse_since(raw: &str) -> anyhow::Result<UnixMillis> {
    if let Ok(ms) = raw.parse::<i64>() {
        return Ok(UnixMillis(ms));
    }
    let (number, unit) = raw.split_at(raw.len().saturating_sub(1));
    let n: i64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("--since: use 30m, 2h, 7d or Unix ms"))?;
    let ms = match unit {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => bail!("--since: use 30m, 2h, 7d or Unix ms"),
    };
    Ok(UnixMillis(UnixMillis::now().0 - ms))
}

fn when(at: UnixMillis) -> String {
    jiff::Timestamp::from_millisecond(at.0)
        .map(|ts| {
            ts.to_zoned_datetime(
                jiff::tz::TimeZone::try_system().unwrap_or(jiff::tz::TimeZone::UTC),
            )
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string()
        })
        .unwrap_or_else(|_| at.0.to_string())
}

pub async fn audit(runtime: &Runtime, command: AuditCommand) -> anyhow::Result<()> {
    match command {
        AuditCommand::List { tool, since, limit } => {
            let filter = ToolRunFilter {
                tool,
                since: since.as_deref().map(parse_since).transpose()?,
                limit: Some(limit),
            };
            let runs = runtime.audit_runs(filter).await?;
            if runs.is_empty() {
                println!("No tool runs recorded.");
            }
            for run in runs {
                println!(
                    "{}  {:<13} {:<10} {:>6} ms  task {}  {}",
                    when(run.created_at),
                    run.tool,
                    run.status,
                    run.duration_ms,
                    run.task_id,
                    run.redacted_result.unwrap_or_default()
                );
            }
        }
        AuditCommand::Show { task_id } => {
            let (task, runs) = runtime.audit_task(&TaskId::new(task_id)).await?;
            println!("Task:    {}", task.id);
            println!("State:   {}", task.state.as_str());
            println!(
                "Created: {}   Updated: {}",
                when(task.created_at),
                when(task.updated_at)
            );
            println!("Request (redacted): {}", task.request_json);
            for run in runs {
                println!(
                    "Run:     {} {} in {} ms: {}",
                    run.tool,
                    run.status,
                    run.duration_ms,
                    run.redacted_result.unwrap_or_default()
                );
            }
        }
    }
    Ok(())
}
