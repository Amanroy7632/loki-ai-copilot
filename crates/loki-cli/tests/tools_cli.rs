//! P3 end-to-end through the real `loki` binary: permissions, roots, a scripted tool turn with
//! its audit trail, "audit output contains no file contents" (P3-T09), and a rejected approval.

// Test helpers outside #[test] fns aren't covered by clippy.toml's allow-*-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

const SECRET: &str = "TOP-SECRET-CONTENT-4242";

fn loki(data_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loki"));
    command
        .arg("--data-dir")
        .arg(data_dir)
        .args(["--provider", "mock"])
        .env_remove("LOKI_DATA_DIR");
    command
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "loki failed: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn run(data_dir: &Path, args: &[&str]) -> String {
    ok(loki(data_dir).args(args).output().unwrap())
}

/// `loki` with the mock platform: fake apps, nothing launched, same on every OS.
fn loki_mock_platform(data_dir: &Path) -> Command {
    let mut command = loki(data_dir);
    command.arg("--mock-platform");
    command
}

fn run_mock_platform(data_dir: &Path, args: &[&str]) -> String {
    ok(loki_mock_platform(data_dir).args(args).output().unwrap())
}

fn write_script(dir: &Path, scripts: serde_json::Value) -> std::path::PathBuf {
    let path = dir.join("script.json");
    std::fs::write(&path, json!({ "scripts": scripts }).to_string()).unwrap();
    path
}

fn chat(data_dir: &Path, script: &Path, stdin: &str) -> String {
    let mut child = loki_mock_platform(data_dir)
        .arg("--mock-script")
        .arg(script)
        .arg("chat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    ok(child.wait_with_output().unwrap())
}

#[test]
fn permissions_start_empty_and_grants_are_validated() {
    let dir = tempfile::tempdir().unwrap();
    let list = run(dir.path(), &["permissions", "list"]);
    assert!(
        list.contains("clock") && list.contains("supported"),
        "{list}"
    );
    assert!(
        list.lines().filter(|l| l.contains(" - ")).count() >= 4,
        "nothing is granted by default:\n{list}"
    );

    run(dir.path(), &["permissions", "grant", "clock"]);
    let list = run(dir.path(), &["permissions", "list"]);
    assert!(
        list.lines()
            .any(|l| l.starts_with("clock") && l.contains('*')),
        "{list}"
    );

    // Unknown capability, unavailable capability, and a scope that isn't a root are refused.
    for args in [
        vec!["permissions", "grant", "root_shell"],
        vec!["permissions", "grant", "network"],
        vec!["permissions", "grant", "file_read", "--scope", "nowhere"],
    ] {
        let output = loki(dir.path()).args(&args).output().unwrap();
        assert!(!output.status.success(), "{args:?} should fail");
    }
    assert!(run(dir.path(), &["permissions", "revoke", "clock"]).contains("Revoked 1"));
}

#[test]
fn roots_can_be_added_listed_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    run(
        dir.path(),
        &["roots", "add", "docs", docs.path().to_str().unwrap()],
    );
    assert!(run(dir.path(), &["roots", "list"]).contains("docs"));
    run(
        dir.path(),
        &["permissions", "grant", "file_read", "--scope", "docs"],
    );
    run(dir.path(), &["roots", "remove", "docs"]);
    assert!(run(dir.path(), &["roots", "list"]).contains("No allowed folders"));
    assert!(
        !loki(dir.path())
            .args(["roots", "add", "Bad/Id", "/tmp"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn mock_scripted_tool_turn_is_executed_and_audited() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["permissions", "grant", "clock"]);
    let script = write_script(
        dir.path(),
        json!([
            { "steps": [{ "type": "tool_call", "tool": "time.now", "arguments": { "timezone": "Asia/Tokyo" } }, { "type": "done" }] },
            { "steps": [{ "type": "token", "text": "It is morning in Tokyo." }, { "type": "done" }] }
        ]),
    );
    let out = chat(dir.path(), &script, "what time is it in Tokyo?\n/quit\n");
    assert!(out.contains("[tool time.now: succeeded]"), "{out}");
    assert!(out.contains("It is morning in Tokyo."), "{out}");
    let audit = run(dir.path(), &["audit", "list"]);
    assert!(
        audit.contains("time.now")
            && audit.contains("succeeded")
            && audit.contains("time in Asia/Tokyo"),
        "{audit}"
    );
    let filtered = run(
        dir.path(),
        &["audit", "list", "--tool", "app.open", "--since", "1h"],
    );
    assert!(filtered.contains("No tool runs"), "{filtered}");
}

/// P3-T09 acceptance: audit output contains no file contents (and neither does the database).
#[test]
fn mock_audit_output_and_database_contain_no_file_contents() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tempfile::tempdir().unwrap();
    std::fs::write(
        docs.path().join("notes.md"),
        format!("Meeting notes.\n{SECRET}\n"),
    )
    .unwrap();
    run(
        dir.path(),
        &["roots", "add", "docs", docs.path().to_str().unwrap()],
    );
    run(
        dir.path(),
        &["permissions", "grant", "file_read", "--scope", "docs"],
    );
    let script = write_script(
        dir.path(),
        json!([
            { "steps": [{ "type": "tool_call", "tool": "files.search", "arguments": { "root_id": "docs", "query": "notes" } }, { "type": "done" }] },
            { "steps": [{ "type": "tool_call", "tool": "files.read", "arguments": { "file_id": "$FILE_ID", "max_bytes": 4096 } }, { "type": "done" }] },
            { "steps": [{ "type": "token", "text": "Your notes mention a meeting." }, { "type": "done" }] }
        ]),
    );
    let out = chat(dir.path(), &script, "summarize my notes\n/quit\n");
    assert!(out.contains("[tool files.read: succeeded]"), "{out}");

    let audit = run(dir.path(), &["audit", "list"]);
    assert!(
        audit.contains("files.read")
            && audit.contains("read ")
            && audit.contains("bytes from root 'docs'"),
        "{audit}"
    );
    assert!(!audit.contains(SECRET), "{audit}");
    for line in audit.lines() {
        let task = line
            .split("task ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        let shown = run(dir.path(), &["audit", "show", task]);
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(
            !shown.contains(docs.path().to_str().unwrap()),
            "no absolute paths: {shown}"
        );
    }
    // The secret is only in the source file: not in the database, its WAL, or the logs.
    for entry in walk(dir.path()) {
        if entry.extension().is_some_and(|e| e == "json") && entry.ends_with("script.json") {
            continue;
        }
        let bytes = std::fs::read(&entry).unwrap();
        assert!(
            !contains(&bytes, SECRET.as_bytes()),
            "secret found in {}",
            entry.display()
        );
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The real approval prompt: answering `n` means the app is not launched and the task is
/// recorded as cancelled. Uses the mock platform so it runs on every OS (the approve path on the
/// real macOS adapter is the manual smoke test, because it really launches an app).
#[test]
fn rejected_approval_at_the_prompt_launches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    run_mock_platform(dir.path(), &["permissions", "grant", "app_launch"]);
    let script = write_script(
        dir.path(),
        json!([
            { "steps": [{ "type": "tool_call", "tool": "app.open", "arguments": { "app_id": "com.example.calculator" } }, { "type": "done" }] },
            { "steps": [{ "type": "token", "text": "Okay, not opening it." }, { "type": "done" }] }
        ]),
    );
    let mut child = loki_mock_platform(dir.path())
        .arg("--mock-script")
        .arg(&script)
        .arg("chat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let mut all = String::new();
        let mut buffer = [0_u8; 256];
        while let Ok(n) = stdout.read(&mut buffer) {
            if n == 0 {
                break;
            }
            all.push_str(&String::from_utf8_lossy(&buffer[..n]));
            let _ = sender.send(all.clone());
        }
        all
    });
    writeln!(stdin, "open the calculator").unwrap();
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .expect("approval prompt never appeared")
        .contains("Approve? [y/N]")
    {}
    writeln!(stdin, "n").unwrap();
    writeln!(stdin, "/quit").unwrap();
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let out = reader.join().unwrap();
    assert!(
        out.contains("Tool:      app.open")
            && out.contains("com.example.calculator")
            && out.contains("LocalChange"),
        "{out}"
    );
    assert!(out.contains("Not approved."), "{out}");
    assert!(out.contains("Okay, not opening it."), "{out}");
    let audit = run(dir.path(), &["audit", "list"]);
    assert!(
        audit.contains("app.open") && audit.contains("cancelled"),
        "{audit}"
    );
}

/// Typed-ahead input can't answer an approval: with stdin already closed, it is a no.
#[test]
fn approval_with_no_input_left_is_a_no() {
    let dir = tempfile::tempdir().unwrap();
    run_mock_platform(dir.path(), &["permissions", "grant", "app_launch"]);
    let script = write_script(
        dir.path(),
        json!([
            { "steps": [{ "type": "tool_call", "tool": "app.open", "arguments": { "app_id": "com.example.calculator" } }, { "type": "done" }] },
            { "steps": [{ "type": "token", "text": "ok" }, { "type": "done" }] }
        ]),
    );
    // "y" arrives before the prompt is shown, then stdin closes.
    let out = chat(dir.path(), &script, "open calculator\ny\n");
    assert!(
        out.contains("Not approved") || out.contains("not approved"),
        "{out}"
    );
    assert!(run(dir.path(), &["audit", "list"]).contains("cancelled"));
}

/// Honest capability reporting: where the native adapter can't launch apps (Windows/Linux until
/// P6), granting app_launch is refused rather than shown as working.
#[cfg(not(target_os = "macos"))]
#[test]
fn native_app_launch_is_reported_unavailable_until_p6() {
    let dir = tempfile::tempdir().unwrap();
    let list = run(dir.path(), &["permissions", "list"]);
    assert!(
        list.lines()
            .any(|l| l.starts_with("app_launch") && l.contains("unavailable")),
        "{list}"
    );
    let output = loki(dir.path())
        .args(["permissions", "grant", "app_launch"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unavailable"));
}
