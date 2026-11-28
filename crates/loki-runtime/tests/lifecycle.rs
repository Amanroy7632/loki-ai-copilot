//! P5-T07: emergency stop, quit (shutdown) and start at login, through the real runtime with
//! the labelled mock provider and a `MockAutostart` OS backend.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use loki_core::{CapabilityStatus, TurnEvent};
use loki_llm::{MockScript, MockStep};
use loki_runtime::autostart::{AutostartBackend, AutostartService};
use loki_runtime::{EventSink, MockPlatform, ProviderKind, Runtime, RuntimeOptions};
use loki_tools::platform::Platform;
use serde_json::json;

fn runtime(dir: &tempfile::TempDir, scripts: Option<Vec<MockScript>>) -> Arc<Runtime> {
    let platform = Arc::new(MockPlatform::default());
    Arc::new(
        Runtime::open(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            provider: Some(ProviderKind::Mock),
            platform: Some(platform as Arc<dyn Platform>),
            mock_scripts: scripts,
            mock_token_delay: Duration::from_millis(5),
            ..RuntimeOptions::default()
        })
        .unwrap(),
    )
}

fn collect() -> (EventSink, Arc<Mutex<Vec<TurnEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let store = Arc::clone(&events);
    (
        Arc::new(move |event| store.lock().unwrap().push(event)),
        events,
    )
}

type Running = (
    tokio::task::JoinHandle<Result<(), loki_runtime::RuntimeError>>,
    Arc<Mutex<Vec<TurnEvent>>>,
);

/// Starts a long mock reply (300 words, 5 ms each) in the background.
fn long_turn(rt: &Arc<Runtime>, id: &'static str) -> Running {
    let (sink, events) = collect();
    let running = Arc::clone(rt);
    let text = vec!["word"; 300].join(" ");
    let handle = tokio::spawn(async move { running.chat_turn(id, None, Some(text), sink).await });
    (handle, events)
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_emergency_stop_stops_the_running_reply_and_the_next_one_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, None);
    let control = rt.lifecycle_control();
    let (turn, events) = long_turn(&rt, "t1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.emergency_stop();
    tokio::time::timeout(Duration::from_secs(5), turn)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(TurnEvent::Stopped { .. })
    ));
    assert!(control.wait_for_idle(Duration::from_secs(1)).await);

    let (sink, events) = collect();
    rt.chat_turn("t2", None, Some("hello".into()), sink)
        .await
        .unwrap();
    assert!(matches!(
        events.lock().unwrap().last(),
        Some(TurnEvent::Completed { .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_quit_stops_running_work_waits_for_it_and_refuses_new_turns() {
    let dir = tempfile::tempdir().unwrap();
    let rt = runtime(&dir, None);
    let control = rt.lifecycle_control();
    let (turn, events) = long_turn(&rt, "t1");
    tokio::time::sleep(Duration::from_millis(100)).await;
    control.begin_shutdown();
    assert!(
        control.wait_for_idle(Duration::from_secs(2)).await,
        "the running turn finished cleanly"
    );
    turn.await.unwrap().unwrap();
    // The partial reply was stored before the app exits.
    let Some(TurnEvent::Stopped { message: Some(m) }) = events.lock().unwrap().last().cloned()
    else {
        panic!("expected a stored partial reply");
    };
    assert!(m.stopped);

    let (sink, _) = collect();
    assert!(
        rt.chat_turn("t2", None, Some("hello".into()), sink)
            .await
            .is_err(),
        "no new turns after quit"
    );
    control.begin_shutdown(); // repeatable
}

#[tokio::test(flavor = "multi_thread")]
async fn mock_quit_while_an_approval_waits_denies_it_and_nothing_runs() {
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
    ];
    let dir = tempfile::tempdir().unwrap();
    let platform = Arc::new(MockPlatform::default());
    let rt = Arc::new(
        Runtime::open(RuntimeOptions {
            data_dir: Some(dir.path().to_path_buf()),
            provider: Some(ProviderKind::Mock),
            platform: Some(Arc::clone(&platform) as Arc<dyn Platform>),
            mock_scripts: Some(scripts),
            ..RuntimeOptions::default()
        })
        .unwrap(),
    );
    rt.grant("app_launch", "*").await.unwrap();
    let control = rt.lifecycle_control();
    let (sink, events) = collect();
    let running = Arc::clone(&rt);
    let turn = tokio::spawn(async move {
        running
            .chat_turn("t1", None, Some("open it".into()), sink)
            .await
    });
    // Wait until the approval is pending, then quit without answering.
    for _ in 0..200 {
        let pending = events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, TurnEvent::ApprovalRequested { .. }));
        if pending {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    control.begin_shutdown();
    assert!(control.wait_for_idle(Duration::from_secs(2)).await);
    let _ = turn.await.unwrap();
    assert!(platform.launches().is_empty(), "nothing was launched");
}

/// A labelled stand-in for the OS login-items API.
#[derive(Default)]
struct MockAutostart {
    enabled: Mutex<bool>,
    read_error: Option<String>,
    ignore_writes: bool,
    writes: Arc<Mutex<Vec<bool>>>,
}

impl AutostartBackend for MockAutostart {
    fn is_enabled(&self) -> Result<bool, String> {
        match &self.read_error {
            Some(error) => Err(error.clone()),
            None => Ok(*self.enabled.lock().unwrap()),
        }
    }
    fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        self.writes.lock().unwrap().push(enabled);
        if !self.ignore_writes {
            *self.enabled.lock().unwrap() = enabled;
        }
        Ok(())
    }
}

#[tokio::test]
async fn mock_autostart_is_off_until_the_user_turns_it_on_and_reports_the_os_state() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let service = AutostartService::new(
        MockAutostart {
            writes: Arc::clone(&writes),
            ..MockAutostart::default()
        },
        None,
    );
    let status = service.status().await;
    assert_eq!(status.enabled, Some(false));
    assert_eq!(status.availability, CapabilityStatus::Supported);
    assert!(writes.lock().unwrap().is_empty(), "reading never registers");

    assert_eq!(service.set_enabled(true).await.unwrap().enabled, Some(true));
    assert_eq!(service.status().await.enabled, Some(true));
    assert_eq!(
        service.set_enabled(false).await.unwrap().enabled,
        Some(false)
    );
    assert_eq!(*writes.lock().unwrap(), [true, false]);
}

#[tokio::test]
async fn mock_autostart_failures_are_visible_and_never_shown_as_off() {
    // Unavailable (e.g. a development build): no OS call at all.
    let writes = Arc::new(Mutex::new(Vec::new()));
    let unavailable = AutostartService::new(
        MockAutostart {
            writes: Arc::clone(&writes),
            ..MockAutostart::default()
        },
        Some("development build".into()),
    );
    let status = unavailable.status().await;
    assert_eq!(status.enabled, None);
    assert!(matches!(
        status.availability,
        CapabilityStatus::Unavailable { .. }
    ));
    assert!(unavailable.set_enabled(true).await.is_err());
    assert!(writes.lock().unwrap().is_empty());

    // The OS can't be read: unknown, not "off".
    let unreadable = AutostartService::new(
        MockAutostart {
            read_error: Some("login items unavailable".into()),
            ..MockAutostart::default()
        },
        None,
    );
    let status = unreadable.status().await;
    assert_eq!(status.enabled, None);
    assert!(matches!(
        status.availability,
        CapabilityStatus::Denied { .. }
    ));

    // The OS accepted the call but didn't change: reported as a failure.
    let ignored = AutostartService::new(
        MockAutostart {
            ignore_writes: true,
            ..MockAutostart::default()
        },
        None,
    );
    let error = ignored.set_enabled(true).await.unwrap_err();
    assert!(error.contains("did not apply"), "{error}");
}
