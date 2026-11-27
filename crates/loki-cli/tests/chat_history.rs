//! P1-T08 acceptance: a mock chat is persisted, and after restarting the CLI `history show`
//! prints it. Each test drives the real `loki` binary against its own temp data dir.

// Test helpers outside #[test] fns aren't covered by clippy.toml's allow-*-in-tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn loki(data_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loki"));
    command
        .arg("--data-dir")
        .arg(data_dir)
        // Make sure the developer's own override can't leak into tests.
        .env_remove("LOKI_DATA_DIR")
        .env("LOKI_LOG", "debug");
    command
}

fn run_with_stdin(mut command: Command, stdin: &str) -> Output {
    let mut child = command
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
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "loki failed: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn only_conversation_id(data_dir: &Path) -> String {
    let mut command = loki(data_dir);
    command.args(["history", "list"]);
    let listing = stdout(&command.output().unwrap());
    let lines: Vec<&str> = listing.lines().collect();
    assert_eq!(lines.len(), 1, "expected one conversation: {listing}");
    lines[0].split_whitespace().next().unwrap().to_owned()
}

fn history_show(data_dir: &Path, id: &str) -> String {
    let mut command = loki(data_dir);
    command.args(["history", "show", id]);
    stdout(&command.output().unwrap())
}

#[test]
fn mock_chat_is_persisted_and_shown_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut chat = loki(dir.path());
    chat.arg("chat");
    let session = stdout(&run_with_stdin(chat, "hello loki\n/quit\n"));
    assert!(session.contains("Provider: mock"), "{session}");
    assert!(session.contains("Mock reply: hello loki"), "{session}");

    // New processes: list, then show.
    let id = only_conversation_id(dir.path());
    assert!(
        session.contains(&id),
        "chat should print the saved id: {session}"
    );
    let shown = history_show(dir.path(), &id);
    assert!(shown.contains("# hello loki"), "{shown}");
    assert!(shown.contains("[user] hello loki"), "{shown}");
    assert!(
        shown.contains("[assistant] Mock reply: hello loki"),
        "{shown}"
    );

    // Logs go to the data dir, not the working directory.
    let logs: Vec<_> = std::fs::read_dir(dir.path().join("logs"))
        .unwrap()
        .collect();
    assert!(!logs.is_empty(), "no log file written");
    assert!(dir.path().join("loki.db").is_file());
    assert!(dir.path().join("config").join("config.json").is_file());
}

#[test]
fn mock_chat_can_resume_a_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = loki(dir.path());
    first.arg("chat");
    stdout(&run_with_stdin(first, "first question\n/quit\n"));
    let id = only_conversation_id(dir.path());

    let mut second = loki(dir.path());
    second.args(["chat", "--conversation", &id]);
    let session = stdout(&run_with_stdin(second, "second question\n/quit\n"));
    assert!(
        session.contains("Continuing \"first question\""),
        "{session}"
    );

    assert_eq!(
        only_conversation_id(dir.path()),
        id,
        "must not create a new conversation"
    );
    let shown = history_show(dir.path(), &id);
    let lines: Vec<&str> = shown.lines().skip(1).collect();
    assert_eq!(
        lines,
        [
            "[user] first question",
            "[assistant] Mock reply: first question",
            "[user] second question",
            "[assistant] Mock reply: second question",
        ]
    );
}

#[test]
fn mock_chat_retry_replaces_the_last_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut chat = loki(dir.path());
    chat.arg("chat");
    stdout(&run_with_stdin(chat, "question\n/retry\n/retry\n/quit\n"));
    let shown = history_show(dir.path(), &only_conversation_id(dir.path()));
    assert_eq!(shown.matches("[assistant]").count(), 1, "{shown}");
    assert_eq!(shown.matches("[user]").count(), 1, "{shown}");
}

#[test]
fn end_of_input_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut chat = loki(dir.path());
    chat.arg("chat");
    stdout(&run_with_stdin(chat, "no quit command\n"));
    let shown = history_show(dir.path(), &only_conversation_id(dir.path()));
    assert!(
        shown.contains("[assistant] Mock reply: no quit command"),
        "{shown}"
    );
}

#[test]
fn unknown_conversation_fails_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut show = loki(dir.path());
    show.args(["history", "show", "does-not-exist"]);
    let output = show.output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not found"));

    let mut chat = loki(dir.path());
    chat.args(["chat", "--conversation", "does-not-exist"]);
    let output = run_with_stdin(chat, "/quit\n");
    assert!(!output.status.success());
}

#[test]
fn empty_history_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut list = loki(dir.path());
    list.args(["history", "list"]);
    assert!(stdout(&list.output().unwrap()).contains("No conversations yet"));
}

/// Ctrl+C (SIGINT) during a reply stops the reply, not the app. Unix-only because sending a
/// console Ctrl+C to a child process on Windows needs platform APIs; covered manually there.
#[cfg(unix)]
#[test]
fn mock_ctrl_c_stops_generation_but_not_the_app() {
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();
    let mut child = loki(dir.path())
        .arg("chat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    // Collect stdout on a thread so we can wait for the reply to start streaming.
    let mut child_stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut all = String::new();
        let mut buffer = [0_u8; 256];
        while let Ok(read) = child_stdout.read(&mut buffer) {
            if read == 0 {
                break;
            }
            all.push_str(&String::from_utf8_lossy(&buffer[..read]));
            let _ = sender.send(all.clone());
        }
        all
    });

    // ~200 words at 40 ms per token: about 8 s of streaming, far longer than the test waits.
    let long = vec!["word"; 200].join(" ");
    writeln!(stdin, "{long}").unwrap();
    stdin.flush().unwrap();

    // Signal only once tokens are visibly streaming: the Ctrl+C handler is installed by then
    // (it is set up at the first prompt), so this doesn't race process startup.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let seen = receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .expect("reply never started streaming");
        if seen.contains("Mock reply: word") {
            break;
        }
    }

    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    std::thread::sleep(Duration::from_millis(300));

    writeln!(stdin, "/quit").unwrap();
    drop(stdin);
    let status = child.wait().unwrap();
    let out = reader.join().unwrap();
    assert!(status.success(), "loki exited with {status}: {out}");
    assert!(out.contains("[stopped; not saved."), "{out}");
    assert!(
        out.contains("Saved as conversation"),
        "app kept running after Ctrl+C: {out}"
    );

    let shown = history_show(dir.path(), &only_conversation_id(dir.path()));
    assert_eq!(shown.matches("[user]").count(), 1, "{shown}");
    assert_eq!(
        shown.matches("[assistant]").count(),
        0,
        "cancelled reply must not be stored: {shown}"
    );
}
