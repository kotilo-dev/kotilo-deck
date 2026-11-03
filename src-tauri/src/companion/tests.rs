//! Mock-desktop integration tests (SPEC §9): a real server on an ephemeral port with a
//! temp token file, driven by a tokio-tungstenite client.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::codec::{Agent, Approval, Decision, Message, PomodoroState};
use super::server::{CompanionSnapshot, CompanionStatus, Config, Host, Server};
use super::token;
use crate::widgets::pomodoro::Phase;

const WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
enum Ev {
    Status(CompanionStatus),
    Snapshot(CompanionSnapshot),
    Approval(String, Option<String>),
    Suggest(String, Option<u32>),
    Switch(String),
}

struct TestHost {
    tx: mpsc::UnboundedSender<Ev>,
    pomodoro: Mutex<PomodoroState>,
}

impl Host for TestHost {
    fn status_changed(&self, s: &CompanionStatus) {
        let _ = self.tx.send(Ev::Status(s.clone()));
    }
    fn snapshot_changed(&self, s: &CompanionSnapshot) {
        let _ = self.tx.send(Ev::Snapshot(s.clone()));
    }
    fn approval_requested(&self, a: &Approval, agent: Option<&Agent>) {
        let _ = self
            .tx
            .send(Ev::Approval(a.id.clone(), agent.map(|g| g.repo.clone())));
    }
    fn suggest(&self, reason: &str, minutes: Option<u32>) {
        let _ = self.tx.send(Ev::Suggest(reason.into(), minutes));
    }
    fn workspace_switch(&self, slug: &str) {
        let _ = self.tx.send(Ev::Switch(slug.into()));
    }
    fn pomodoro_state(&self) -> PomodoroState {
        self.pomodoro.lock().unwrap().clone()
    }
}

fn pomo(phase: Phase, running: bool) -> PomodoroState {
    PomodoroState {
        phase,
        running,
        ends_at: running.then_some(1_793_734_500_000),
        remaining: 1500,
        duration: 1500,
        session: 1,
        sessions_per_cycle: 4,
    }
}

struct Rig {
    server: Server,
    events: mpsc::UnboundedReceiver<Ev>,
    port: u16,
    token: String,
    token_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl Rig {
    async fn start_with(f: impl FnOnce(&mut Config)) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".kotilo").join("companion.token");
        let mut cfg = Config::new(0, path.clone());
        cfg.hello_timeout = Duration::from_millis(400);
        f(&mut cfg);
        let (tx, events) = mpsc::unbounded_channel();
        let host = Arc::new(TestHost {
            tx,
            pomodoro: Mutex::new(pomo(Phase::Idle, false)),
        });
        let server = Server::new(cfg, host);
        server.set_enabled(true);
        let mut rig = Rig {
            server,
            events,
            port: 0,
            token: String::new(),
            token_path: path.clone(),
            _dir: dir,
        };
        let st = rig.status(|s| s.listening).await;
        rig.port = st.port;
        // The server created the token file on start.
        rig.token = token::read(&path).unwrap().expect("token created");
        rig
    }

    async fn start() -> Rig {
        Self::start_with(|_| {}).await
    }

    async fn next(&mut self, mut pred: impl FnMut(&Ev) -> bool) -> Ev {
        tokio::time::timeout(WAIT, async {
            loop {
                let ev = self.events.recv().await.expect("host channel open");
                if pred(&ev) {
                    return ev;
                }
            }
        })
        .await
        .expect("expected host event")
    }

    async fn status(&mut self, mut pred: impl FnMut(&CompanionStatus) -> bool) -> CompanionStatus {
        match self.next(|e| matches!(e, Ev::Status(s) if pred(s))).await {
            Ev::Status(s) => s,
            _ => unreachable!(),
        }
    }

    async fn snapshot(
        &mut self,
        mut pred: impl FnMut(&CompanionSnapshot) -> bool,
    ) -> CompanionSnapshot {
        match self.next(|e| matches!(e, Ev::Snapshot(s) if pred(s))).await {
            Ev::Snapshot(s) => s,
            _ => unreachable!(),
        }
    }

    async fn connect(&self) -> Client {
        let tcp = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        let url = format!("ws://127.0.0.1:{}/", self.port);
        let (ws, _) = tokio_tungstenite::client_async(url, tcp).await.unwrap();
        Client(ws)
    }

    /// Connected + authenticated; consumes `hello.ok` and the initial `pomodoro.state`.
    async fn desktop(&mut self) -> Client {
        let mut c = self.connect().await;
        c.hello(&self.token.clone(), 1).await;
        assert_eq!(c.recv().await["type"], "hello.ok");
        assert_eq!(c.recv().await["type"], "pomodoro.state");
        self.status(|s| s.connected).await;
        c
    }
}

struct Client(WebSocketStream<TcpStream>);

impl Client {
    async fn send(&mut self, v: Value) {
        self.0.send(WsMessage::text(v.to_string())).await.unwrap();
    }

    async fn hello(&mut self, token: &str, v: u64) {
        self.send(json!({"v": v, "type": "hello", "payload": {
            "token": token, "client": "kotilo-desktop", "clientVersion": "1.4.0"}}))
            .await;
    }

    /// Next text frame as JSON.
    async fn recv(&mut self) -> Value {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.0.next().await {
                    Some(Ok(WsMessage::Text(t))) => {
                        return serde_json::from_str(t.as_str()).unwrap()
                    }
                    Some(Ok(WsMessage::Close(f))) => panic!("closed: {f:?}"),
                    Some(Ok(_)) => continue,
                    other => panic!("connection ended: {other:?}"),
                }
            }
        })
        .await
        .expect("expected a frame")
    }

    /// Reads until the close frame; returns its code (None = dropped without one).
    async fn close_code(&mut self) -> Option<u16> {
        tokio::time::timeout(WAIT, async {
            loop {
                match self.0.next().await {
                    Some(Ok(WsMessage::Close(f))) => return f.map(|f| u16::from(f.code)),
                    Some(Ok(_)) => continue,
                    _ => return None,
                }
            }
        })
        .await
        .expect("expected close")
    }
}

fn agents_state() -> Value {
    json!({"v": 1, "type": "agents.state", "payload": {
        "agents": [
            {"id": "a1", "repo": "kotilo-deck", "name": "claude", "status": "working", "task": "Phase 3", "since": 1},
            {"id": "a2", "repo": "shell", "name": "claude", "status": "waiting"}
        ],
        "approvals": [
            {"id": "ap-17", "agentId": "a2", "summary": "Run cargo build", "detail": "cargo build --release", "requestedAt": 2}
        ]
    }})
}

#[tokio::test]
async fn handshake_good_token_gets_hello_ok_then_pomodoro_state() {
    let mut rig = Rig::start().await;
    let mut c = rig.connect().await;
    c.hello(&rig.token.clone(), 1).await;
    let ok = c.recv().await;
    assert_eq!(
        ok,
        json!({"v": 1, "type": "hello.ok", "payload": {
            "server": "kotilo-deck", "serverVersion": env!("CARGO_PKG_VERSION"), "protocol": 1}})
    );
    let p = c.recv().await;
    assert_eq!(p["type"], "pomodoro.state");
    assert_eq!(p["payload"]["phase"], "idle");
    assert_eq!(p["payload"]["endsAt"], Value::Null);
    let st = rig.status(|s| s.connected).await;
    assert_eq!(st.client.as_deref(), Some("1.4.0"));
    assert!(st.enabled && st.listening && st.error.is_none());
    // Readers trim whitespace: a token with a trailing newline is accepted too.
    let mut c2 = rig.connect().await;
    c2.hello(&format!("{}\n", rig.token), 1).await;
    assert_eq!(c2.recv().await["type"], "hello.ok");
}

#[tokio::test]
async fn handshake_rejections_use_protocol_close_codes() {
    let rig = Rig::start().await;
    let bad = "0".repeat(64);

    let mut c = rig.connect().await;
    c.hello(&bad, 1).await;
    assert_eq!(c.close_code().await, Some(4401), "bad token");

    let mut c = rig.connect().await;
    assert_eq!(
        c.close_code().await,
        Some(4401),
        "no hello within the timeout"
    );

    let mut c = rig.connect().await;
    c.hello(&rig.token, 2).await;
    assert_eq!(c.close_code().await, Some(4426), "v: 2");

    let mut c = rig.connect().await;
    c.0.send(WsMessage::text("{not json")).await.unwrap();
    assert_eq!(c.close_code().await, Some(4400), "malformed first frame");

    let mut c = rig.connect().await;
    c.send(agents_state()).await;
    assert_eq!(c.close_code().await, Some(4401), "first frame is not hello");

    let mut c = rig.connect().await;
    c.send(json!({"v": 1, "type": "hello", "payload": {}}))
        .await;
    assert_eq!(c.close_code().await, Some(4401), "hello without token");

    assert!(!rig.server.status().connected);
}

#[tokio::test]
async fn second_client_replaces_the_first_with_4409() {
    let mut rig = Rig::start().await;
    let mut first = rig.desktop().await;
    first.send(agents_state()).await;
    rig.snapshot(|s| s.agents.len() == 2).await;

    let mut second = rig.connect().await;
    second.hello(&rig.token.clone(), 1).await;
    assert_eq!(second.recv().await["type"], "hello.ok");
    assert_eq!(first.close_code().await, Some(4409));
    // The old client's snapshot is gone; the new client is the one connected.
    rig.snapshot(|s| s.agents.is_empty()).await;
    assert_eq!(second.recv().await["type"], "pomodoro.state");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(rig.server.status().connected);
    assert!(rig
        .server
        .send(&Message::PomodoroState(pomo(Phase::Focus, true))));
    assert_eq!(second.recv().await["payload"]["phase"], "focus");
}

#[tokio::test]
async fn snapshot_and_approvals_flow() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;

    c.send(agents_state()).await;
    let s = rig.snapshot(|s| !s.agents.is_empty()).await;
    assert_eq!(s.agents[1].task, None);
    assert_eq!(s.approvals[0].id, "ap-17");
    assert!(s.usage.is_none() && s.updated_at.is_some());

    c.send(json!({"v": 1, "type": "usage.today", "payload": {"tokens": 1840000, "costUsd": 4.12}}))
        .await;
    let s = rig.snapshot(|s| s.usage.is_some()).await;
    assert_eq!(s.usage.unwrap().budget_usd, None);

    c.send(
        json!({"v": 1, "type": "agent.approval.requested", "payload":
        {"id": "ap-18", "agentId": "a1", "summary": "Push to main", "requestedAt": 3}}),
    )
    .await;
    let s = rig.snapshot(|s| s.approvals.len() == 2).await;
    assert_eq!(s.approvals[1].detail, None);
    match rig.next(|e| matches!(e, Ev::Approval(..))).await {
        Ev::Approval(id, repo) => {
            assert_eq!(id, "ap-18");
            assert_eq!(repo.as_deref(), Some("kotilo-deck"));
        }
        _ => unreachable!(),
    }

    c.send(json!({"v": 1, "type": "agent.approval.resolved", "payload":
        {"id": "ap-17", "decision": "approve", "by": "desktop"}}))
        .await;
    let s = rig.snapshot(|s| s.approvals.len() == 1).await;
    assert_eq!(s.approvals[0].id, "ap-18");
    assert_eq!(rig.server.snapshot().approvals.len(), 1);

    // Disconnect clears the snapshot.
    drop(c);
    // Status is announced before the cleared snapshot.
    rig.status(|s| !s.connected).await;
    rig.snapshot(|s| *s == CompanionSnapshot::default()).await;
}

#[tokio::test]
async fn respond_reaches_the_desktop_and_removes_locally() {
    let mut rig = Rig::start().await;
    assert!(rig.server.respond("ap-17", Decision::Approve).is_err());
    let mut c = rig.desktop().await;
    c.send(agents_state()).await;
    rig.snapshot(|s| s.approvals.len() == 1).await;

    rig.server.respond("ap-17", Decision::Deny).unwrap();
    assert_eq!(
        c.recv().await,
        json!({"v": 1, "type": "approval.respond", "payload": {"id": "ap-17", "decision": "deny"}})
    );
    rig.snapshot(|s| s.approvals.is_empty()).await;
}

#[tokio::test]
async fn pomodoro_state_on_connect_and_on_change() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;
    assert!(rig
        .server
        .send(&Message::PomodoroState(pomo(Phase::Focus, true))));
    let p = c.recv().await;
    assert_eq!(p["type"], "pomodoro.state");
    assert_eq!(
        p["payload"],
        json!({"phase": "focus", "running": true, "endsAt": 1_793_734_500_000_i64,
               "remaining": 1500, "duration": 1500, "session": 1, "sessionsPerCycle": 4})
    );
    // Not connected → nothing to send, no error.
    drop(c);
    rig.status(|s| !s.connected).await;
    assert!(!rig
        .server
        .send(&Message::PomodoroState(pomo(Phase::Idle, false))));
}

#[tokio::test]
async fn unknown_and_invalid_messages_are_ignored() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;
    c.send(json!({"v": 1, "type": "agents.telemetry", "payload": {"x": 1}}))
        .await;
    c.send(json!({"v": 1, "type": "agents.state", "payload": {"agents": "nope"}}))
        .await;
    c.send(json!({"v": 7, "type": "usage.today", "payload": {}}))
        .await;
    c.0.send(WsMessage::text("garbage")).await.unwrap();
    c.0.send(WsMessage::binary(vec![1, 2, 3])).await.unwrap();
    c.send(json!({"v": 1, "type": "pomodoro.state", "payload": {}}))
        .await;
    // Still connected and processing.
    c.send(agents_state()).await;
    rig.snapshot(|s| s.agents.len() == 2).await;
    assert!(rig.server.status().connected);
}

#[tokio::test]
async fn suggest_and_workspace_switch_reach_the_host() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;
    c.send(json!({"v": 1, "type": "pomodoro.suggest", "payload": {"reason": "Long task", "minutes": 25}}))
        .await;
    match rig.next(|e| matches!(e, Ev::Suggest(..))).await {
        Ev::Suggest(r, m) => assert_eq!((r.as_str(), m), ("Long task", Some(25))),
        _ => unreachable!(),
    }
    c.send(json!({"v": 1, "type": "workspace.switch", "payload": {"slug": "Not A Slug"}}))
        .await;
    c.send(json!({"v": 1, "type": "workspace.switch", "payload": {"slug": "agents"}}))
        .await;
    match rig.next(|e| matches!(e, Ev::Switch(_))).await {
        Ev::Switch(s) => assert_eq!(s, "agents"),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn missed_pongs_drop_the_client_and_live_clients_stay() {
    let mut rig = Rig::start_with(|c| c.ping_interval = Duration::from_millis(80)).await;
    // A client that keeps reading answers pings automatically and stays connected.
    let mut live = rig.desktop().await;
    let reader = tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_millis(600), async {
            while let Some(Ok(_)) = live.0.next().await {}
        })
        .await;
        live
    });
    let _live = reader.await.unwrap();
    assert!(rig.server.status().connected, "pongs keep the client");

    // Now stop reading: no pongs → dropped after 3 missed.
    rig.status(|s| !s.connected).await;
}

#[tokio::test]
async fn oversized_frames_drop_the_connection() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;
    let big = format!(
        r#"{{"v":1,"type":"usage.today","payload":{{"pad":"{}"}}}}"#,
        "x".repeat(super::server::MAX_FRAME + 10)
    );
    let _ = c.0.send(WsMessage::text(big)).await;
    rig.status(|s| !s.connected).await;
}

#[tokio::test]
async fn disabling_closes_with_1001_and_reenabling_listens_again() {
    let mut rig = Rig::start().await;
    let mut c = rig.desktop().await;
    rig.server.set_enabled(false);
    assert_eq!(c.close_code().await, Some(1001));
    let st = rig.status(|s| !s.enabled).await;
    assert!(!st.listening && !st.connected);
    rig.server.set_enabled(true);
    rig.status(|s| s.listening).await;
}

#[tokio::test]
async fn port_in_use_reports_and_retries() {
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::new(port, dir.path().join("companion.token"));
    cfg.retry_base = Duration::from_millis(50);
    cfg.retry_max = Duration::from_millis(100);
    let (tx, mut events) = mpsc::unbounded_channel();
    let server = Server::new(
        cfg,
        Arc::new(TestHost {
            tx,
            pomodoro: Mutex::new(pomo(Phase::Idle, false)),
        }),
    );
    server.set_enabled(true);
    let st = wait_status(&mut events, |s| s.error.is_some()).await;
    assert!(!st.listening);
    assert!(st.error.unwrap().contains(&port.to_string()));
    drop(blocker);
    let st = wait_status(&mut events, |s| s.listening).await;
    assert_eq!(st.port, port);
    assert!(st.error.is_none());
    server.shutdown();
}

async fn wait_status(
    events: &mut mpsc::UnboundedReceiver<Ev>,
    pred: fn(&CompanionStatus) -> bool,
) -> CompanionStatus {
    tokio::time::timeout(WAIT, async {
        loop {
            if let Some(Ev::Status(s)) = events.recv().await {
                if pred(&s) {
                    return s;
                }
            }
        }
    })
    .await
    .expect("status")
}

/// Drives `scripts/mock-desktop.mjs` (the reference client) against a real server:
/// handshake, sample snapshot, `--approve-request`, and the Phase 3 acceptance line when
/// the Deck responds. Needs Node >= 22 on PATH.
#[tokio::test]
#[ignore = "spawns node scripts/mock-desktop.mjs"]
async fn mock_desktop_script_end_to_end() {
    let mut rig = Rig::start().await;
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("mock-desktop.mjs");
    let child = tokio::process::Command::new("node")
        .arg(script)
        .args([
            "--once",
            "--suggest",
            "--switch",
            "agents",
            "--approve-request",
            "Deploy",
        ])
        .arg("--port")
        .arg(rig.port.to_string())
        .arg("--token-file")
        .arg(&rig.token_path)
        .stdout(std::process::Stdio::piped())
        .stdin(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("node on PATH");
    let s = rig
        .snapshot(|s| s.approvals.len() == 2 && s.usage.is_some())
        .await;
    assert_eq!(s.agents.len(), 3);
    rig.next(|e| matches!(e, Ev::Suggest(..))).await;
    rig.next(|e| matches!(e, Ev::Switch(_))).await;
    rig.server.respond("ap-1", Decision::Approve).unwrap();
    // Local optimistic removal, then the script's agent.approval.resolved and agents.state.
    for _ in 0..3 {
        rig.snapshot(|s| s.approvals.len() == 1 && s.approvals[0].summary == "Deploy")
            .await;
    }
    rig.server.set_enabled(false);
    let out = tokio::time::timeout(WAIT, child.wait_with_output())
        .await
        .expect("script exits on close")
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(
            "ACCEPTANCE: approval.respond reached the desktop (id=ap-1, decision=approve)"
        ),
        "{text}"
    );
    assert!(text.contains("<- pomodoro.state"), "{text}");
    assert!(text.contains("closed 1001"), "{text}");
}
