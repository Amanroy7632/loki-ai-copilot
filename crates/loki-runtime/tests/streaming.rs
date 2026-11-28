//! P5-T02: the streaming runtime API with the mock provider: event order, approvals answered
//! through `respond_approval`, and `stop_turn`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use loki_core::{ToolStatus, TurnEvent};
use loki_llm::{MockScript, MockStep};
use loki_runtime::{EventSink, MockPlatform, ProviderKind, Runtime, RuntimeOptions};
use loki_tools::platform::Platform;
use serde_json::json;

fn runtime(
    dir: &tempfile::TempDir,
    scripts: Option<Vec<MockScript>>,
    platform: Arc<MockPlatform>,
) -> Runtime {
    Runtime::open(RuntimeOptions {
        data_dir: Some(dir.path().to_path_buf()),
        provider: Some(ProviderKind::Mock),
        platform: Some(platform as Arc<dyn Platform>),
        mock_scripts: scripts,
        mock_token_delay: Duration::from_millis(5),
        ..RuntimeOptions::default()
    })
    .unwrap()
}

/// Collects events; the test inspects them afterwards.
fn sink() -> (EventSink, Arc<Mutex<Vec<TurnEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::clone(&events);
    (
        Arc::new(move |event| store.lock().unwrap().push(event)),
        events,
    )
}

fn kinds(events: &[TurnEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e {
            TurnEvent::Started { .. } => "started",
            TurnEvent::Token { .. } => "token",
            TurnEvent::ApprovalRequested { .. } => "approval",
            TurnEvent::ToolFinished { .. } => "tool",
            TurnEvent::Completed { .. } => "completed",
            TurnEvent::Stopped { .. } => "stopped",
            TurnEvent::Failed { .. } => "failed",
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_new_conversation_streams_started_tokens_then_completed() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, None, Arc::new(MockPlatform::default()));
    let (sink, events) = sink();
    rt.chat_turn("t1", None, Some(String::from("hello there")), sink)
        .await
        .unwrap();
    let events = events.lock().unwrap().clone();
    let kinds = kinds(&events);
    assert_eq!(kinds.first(), Some(&"started"));
    assert_eq!(kinds.last(), Some(&"completed"));
    assert!(kinds.iter().filter(|k| **k == "token").count() >= 3);
    let streamed: String = events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::Token { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, "Mock reply: hello there");
    let TurnEvent::Completed { message } = events.last().unwrap() else {
        panic!()
    };
    assert_eq!(message.content, streamed);
    let TurnEvent::Started { conversation } = &events[0] else {
        panic!()
    };
    assert_eq!(
        rt.messages(&conversation.id).await.unwrap().len(),
        2,
        "stored"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_approval_is_answered_through_respond_approval() {
    let scripts = vec![
        MockScript::new(vec![
            MockStep::ToolCall {
                tool: "app.open".into(),
                arguments: json!({ "app_id": "com.example.calculator" }),
            },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap(),
        MockScript::new(vec![
            MockStep::Token {
                text: "Opened.".into(),
            },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap(),
    ];
    for approve in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let platform = Arc::new(MockPlatform::default());
        let rt = Arc::new(runtime(&dir, Some(scripts.clone()), Arc::clone(&platform)));
        rt.grant("app_launch", "*").await.unwrap();

        // The "UI": answers the approval when it is shown, as the desktop dialog will.
        let answering = Arc::clone(&rt);
        let shown = Arc::new(Mutex::new(None));
        let shown_store = Arc::clone(&shown);
        let events = Arc::new(Mutex::new(Vec::new()));
        let event_store = Arc::clone(&events);
        let sink: EventSink = Arc::new(move |event: TurnEvent| {
            if let TurnEvent::ApprovalRequested { approval } = &event {
                *shown_store.lock().unwrap() = Some(approval.clone());
                let rt = Arc::clone(&answering);
                let id = approval.id.to_string();
                let shown = approval.arguments.clone();
                tokio::spawn(async move {
                    assert!(rt.respond_approval(&id, approve, &shown));
                    assert!(
                        !rt.respond_approval(&id, approve, &shown),
                        "an answer is single use"
                    );
                });
            }
            event_store.lock().unwrap().push(event);
        });
        rt.chat_turn(
            "t-approve",
            None,
            Some(String::from("open the calculator")),
            sink,
        )
        .await
        .unwrap();

        let approval = shown
            .lock()
            .unwrap()
            .clone()
            .expect("approval was requested");
        assert_eq!(approval.tool.as_str(), "app.open");
        assert_eq!(approval.arguments["app_id"], "com.example.calculator");
        let events = events.lock().unwrap().clone();
        let finished = events.iter().find_map(|e| match e {
            TurnEvent::ToolFinished { status, .. } => Some(*status),
            _ => None,
        });
        if approve {
            assert_eq!(platform.launches(), ["com.example.calculator"]);
            assert_eq!(finished, Some(ToolStatus::Succeeded));
        } else {
            assert!(platform.launches().is_empty());
            assert_eq!(finished, Some(ToolStatus::Cancelled));
        }
        assert!(matches!(events.last(), Some(TurnEvent::Completed { .. })));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_stop_turn_ends_a_running_reply() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Arc::new(runtime(&dir, None, Arc::new(MockPlatform::default())));
    let (sink, events) = sink();
    let long = vec!["word"; 300].join(" ");
    let running = Arc::clone(&rt);
    let turn =
        tokio::spawn(async move { running.chat_turn("t-stop", None, Some(long), sink).await });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(rt.stop_turn("t-stop"));
    tokio::time::timeout(Duration::from_secs(5), turn)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let events = events.lock().unwrap().clone();
    assert!(
        matches!(events.last(), Some(TurnEvent::Stopped { message: Some(m) }) if m.stopped),
        "{:?}",
        kinds(&events)
    );
    assert!(!rt.stop_turn("t-stop"), "finished turns can't be stopped");
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_turn_requests_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, None, Arc::new(MockPlatform::default()));
    let (sink, events) = sink();
    assert!(
        rt.chat_turn("", None, Some("x".into()), Arc::clone(&sink))
            .await
            .is_err()
    );
    assert!(
        rt.chat_turn("t", None, None, Arc::clone(&sink))
            .await
            .is_err()
    );
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(TurnEvent::Failed { .. })
    ));
    assert!(!rt.respond_approval("not-an-approval", true, &json!({})));
}

#[tokio::test(flavor = "multi_thread")]
async fn conversations_can_be_renamed_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, None, Arc::new(MockPlatform::default()));
    let (sink, events) = sink();
    rt.chat_turn("t", None, Some(String::from("first")), sink)
        .await
        .unwrap();
    let TurnEvent::Started { conversation } = events.lock().unwrap()[0].clone() else {
        panic!()
    };

    let renamed = rt
        .rename_conversation(&conversation.id, "  Groceries  ")
        .await
        .unwrap();
    assert_eq!(renamed.title, "Groceries");
    assert!(
        rt.rename_conversation(&conversation.id, "   ")
            .await
            .is_err(),
        "empty titles are refused"
    );
    let long = rt
        .rename_conversation(&conversation.id, &"x".repeat(500))
        .await
        .unwrap();
    assert_eq!(long.title.chars().count(), 200);

    rt.delete_conversation(&conversation.id).await.unwrap();
    assert!(rt.list_conversations().await.unwrap().is_empty());
    assert!(
        rt.messages(&conversation.id).await.is_err(),
        "messages are gone with it"
    );
    assert!(rt.delete_conversation(&conversation.id).await.is_err());
}

/// P5-T04: a "yes" for arguments other than the pending ones (a stale or wrong dialog) is
/// refused, delivered as a refusal, and nothing runs.
#[tokio::test(flavor = "multi_thread")]
async fn mock_approval_for_different_shown_arguments_runs_nothing() {
    let scripts = vec![
        MockScript::new(vec![
            MockStep::ToolCall {
                tool: "app.open".into(),
                arguments: json!({ "app_id": "com.example.calculator" }),
            },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap(),
        MockScript::new(vec![
            MockStep::Token {
                text: "Not opened.".into(),
            },
            MockStep::Done {
                prompt_tokens: None,
                completion_tokens: None,
            },
        ])
        .unwrap(),
    ];
    let dir = tempfile::tempdir().unwrap();
    let platform = Arc::new(MockPlatform::default());
    let rt = Arc::new(runtime(&dir, Some(scripts), Arc::clone(&platform)));
    rt.grant("app_launch", "*").await.unwrap();

    let answering = Arc::clone(&rt);
    let answers = Arc::new(Mutex::new(Vec::new()));
    let answer_store = Arc::clone(&answers);
    let (collect, events) = sink();
    let sink: EventSink = Arc::new(move |event: TurnEvent| {
        if let TurnEvent::ApprovalRequested { approval } = &event {
            let rt = Arc::clone(&answering);
            let id = approval.id.to_string();
            let answer_store = Arc::clone(&answer_store);
            tokio::spawn(async move {
                let wrong = json!({ "app_id": "com.example.other" });
                answer_store
                    .lock()
                    .unwrap()
                    .push(rt.respond_approval(&id, true, &wrong));
            });
        }
        collect(event);
    });
    rt.chat_turn("t-wrong", None, Some(String::from("open it")), sink)
        .await
        .unwrap();

    assert_eq!(*answers.lock().unwrap(), [false], "the answer is refused");
    assert!(platform.launches().is_empty(), "nothing was launched");
    let events = events.lock().unwrap().clone();
    let finished = events.iter().find_map(|e| match e {
        TurnEvent::ToolFinished { status, .. } => Some(*status),
        _ => None,
    });
    assert_eq!(finished, Some(ToolStatus::Cancelled));
    assert!(matches!(events.last(), Some(TurnEvent::Completed { .. })));
}
