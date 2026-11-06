//! The companion WebSocket server (protocol.md): loopback only, one authenticated desktop
//! client at a time, token handshake, keep-alive pings, 1 MiB frame cap.
//!
//! Independent of Tauri: side effects go through [`Host`], and the port, token path and
//! timings come from [`Config`], so tests run a real server on an ephemeral port.
//! Nothing here is fatal: bind failures retry with backoff, bad frames are logged (type
//! and error position only; never tokens or approval text) and ignored.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::time::{timeout, timeout_at, Instant};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::codec::{
    self, Agent, Approval, AuthMode, Decision, DecodeError, Decoded, HelloChallenge, HelloOk,
    Message, PomodoroState, Usage,
};
use super::token;

pub const MAX_FRAME: usize = 1 << 20;
/// Connections still in the handshake; more are dropped at once (local DoS guard).
const MAX_PENDING: usize = 16;

pub mod close {
    pub const GOING_AWAY: u16 = 1001;
    pub const MALFORMED: u16 = 4400;
    pub const UNAUTHORIZED: u16 = 4401;
    pub const REPLACED: u16 = 4409;
    pub const UNSUPPORTED_VERSION: u16 = 4426;
}

/// `CompanionStatus` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionStatus {
    pub enabled: bool,
    pub listening: bool,
    pub connected: bool,
    pub client: Option<String>,
    pub port: u16,
    pub error: Option<String>,
}

/// `CompanionSnapshot` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionSnapshot {
    pub agents: Vec<Agent>,
    pub approvals: Vec<Approval>,
    pub usage: Option<Usage>,
    pub updated_at: Option<i64>,
}

/// Side effects of the server, implemented by the Tauri glue (and by tests).
pub trait Host: Send + Sync + 'static {
    fn status_changed(&self, status: &CompanionStatus);
    fn snapshot_changed(&self, snapshot: &CompanionSnapshot);
    /// A new approval arrived (once per id per connection); `agent` if known.
    fn approval_requested(&self, approval: &Approval, agent: Option<&Agent>);
    fn suggest(&self, reason: &str, minutes: Option<u32>);
    /// `workspace.switch` with a syntactically valid slug; the host applies the user setting.
    fn workspace_switch(&self, slug: &str);
    /// Sent right after `hello.ok`.
    fn pomodoro_state(&self) -> PomodoroState;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub token_path: PathBuf,
    pub server_version: String,
    pub hello_timeout: Duration,
    pub ping_interval: Duration,
    pub max_missed_pongs: u32,
    /// First bind retry delay; doubles up to `retry_max`.
    pub retry_base: Duration,
    pub retry_max: Duration,
}

impl Config {
    pub fn new(port: u16, token_path: PathBuf) -> Self {
        Self {
            port,
            token_path,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            hello_timeout: Duration::from_secs(5),
            ping_interval: Duration::from_secs(15),
            max_missed_pongs: 3,
            retry_base: Duration::from_secs(30),
            retry_max: Duration::from_secs(300),
        }
    }

    fn retry_delay(&self, attempt: u32) -> Duration {
        self.retry_base
            .saturating_mul(1u32 << attempt.min(16))
            .min(self.retry_max)
    }
}

enum Out {
    Text(String),
    Close(u16, &'static str),
}

struct Client {
    conn: u64,
    tx: mpsc::UnboundedSender<Out>,
}

struct State {
    status: CompanionStatus,
    snapshot: CompanionSnapshot,
    client: Option<Client>,
    /// Current listener run: generation + stop signal.
    run: Option<(u64, watch::Sender<bool>)>,
    next_id: u64,
    /// Approval ids already announced on this connection.
    notified: HashSet<String>,
}

struct Shared {
    config: Config,
    host: Arc<dyn Host>,
    state: Mutex<State>,
    pending: AtomicUsize,
}

/// Cheap to clone; all clones drive the same server.
#[derive(Clone)]
pub struct Server {
    shared: Arc<Shared>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Deferred host notifications, fired after the state lock is released.
#[derive(Default)]
struct Effects {
    status: Option<CompanionStatus>,
    snapshot: Option<CompanionSnapshot>,
}

impl Server {
    pub fn new(config: Config, host: Arc<dyn Host>) -> Self {
        let status = CompanionStatus {
            enabled: false,
            listening: false,
            connected: false,
            client: None,
            port: config.port,
            error: None,
        };
        Self {
            shared: Arc::new(Shared {
                config,
                host,
                state: Mutex::new(State {
                    status,
                    snapshot: CompanionSnapshot::default(),
                    client: None,
                    run: None,
                    next_id: 1,
                    notified: HashSet::new(),
                }),
                pending: AtomicUsize::new(0),
            }),
        }
    }

    pub fn status(&self) -> CompanionStatus {
        self.shared.lock().status.clone()
    }

    pub fn snapshot(&self) -> CompanionSnapshot {
        self.shared.lock().snapshot.clone()
    }

    /// Starts or stops listening. Stopping closes the client with `1001`.
    pub fn set_enabled(&self, on: bool) {
        let mut fx = Effects::default();
        {
            let mut st = self.shared.lock();
            if on {
                if st.run.is_some() {
                    return;
                }
                let gen = st.next_id;
                st.next_id += 1;
                let (tx, rx) = watch::channel(false);
                st.run = Some((gen, tx));
                st.status.enabled = true;
                fx.status = Some(st.status.clone());
                let shared = self.shared.clone();
                tauri::async_runtime::spawn(listen(shared, gen, rx));
            } else {
                let Some((_, stop)) = st.run.take() else {
                    return;
                };
                let _ = stop.send(true);
                if let Some(c) = st.client.take() {
                    let _ =
                        c.tx.send(Out::Close(close::GOING_AWAY, "deck companion stopped"));
                }
                st.status = CompanionStatus {
                    enabled: false,
                    listening: false,
                    connected: false,
                    client: None,
                    port: self.shared.config.port,
                    error: None,
                };
                fx.status = Some(st.status.clone());
                fx.snapshot = clear_snapshot(&mut st);
            }
        }
        self.shared.fire(fx);
    }

    /// Deck shutting down: like disabling, the client gets `1001`.
    pub fn shutdown(&self) {
        self.set_enabled(false);
    }

    /// Fire-and-forget to the connected desktop; false if none.
    pub fn send(&self, msg: &Message) -> bool {
        let st = self.shared.lock();
        match &st.client {
            Some(c) => c.tx.send(Out::Text(msg.encode())).is_ok(),
            None => false,
        }
    }

    /// `approval.respond`; the approval is removed locally at once (the desktop's
    /// `agent.approval.resolved` / next `agents.state` corrects it if needed).
    pub fn respond(&self, id: &str, decision: Decision) -> Result<(), String> {
        let msg = Message::ApprovalRespond(codec::ApprovalRespond {
            id: id.to_string(),
            decision,
        });
        let mut fx = Effects::default();
        {
            let mut st = self.shared.lock();
            let sent = st
                .client
                .as_ref()
                .is_some_and(|c| c.tx.send(Out::Text(msg.encode())).is_ok());
            if !sent {
                return Err("Kotilo desktop is not connected".into());
            }
            let before = st.snapshot.approvals.len();
            st.snapshot.approvals.retain(|a| a.id != id);
            if st.snapshot.approvals.len() != before {
                fx.snapshot = Some(st.snapshot.clone());
            }
        }
        tracing::info!(
            approval = id,
            ?decision,
            "approval response sent to desktop"
        );
        self.shared.fire(fx);
        Ok(())
    }
}

fn clear_snapshot(st: &mut State) -> Option<CompanionSnapshot> {
    st.notified.clear();
    let empty = CompanionSnapshot::default();
    (st.snapshot != empty).then(|| {
        st.snapshot = empty;
        st.snapshot.clone()
    })
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn fire(&self, fx: Effects) {
        if let Some(s) = fx.status {
            self.host.status_changed(&s);
        }
        if let Some(s) = fx.snapshot {
            self.host.snapshot_changed(&s);
        }
    }

    /// Updates the status if `gen` is still the live run.
    fn set_status(&self, gen: u64, f: impl FnOnce(&mut CompanionStatus)) {
        let changed = {
            let mut st = self.lock();
            if st.run.as_ref().map(|r| r.0) != Some(gen) {
                return;
            }
            let before = st.status.clone();
            f(&mut st.status);
            (st.status != before).then(|| st.status.clone())
        };
        if let Some(s) = changed {
            self.host.status_changed(&s);
        }
    }

    fn is_current(&self, conn: u64) -> bool {
        self.lock().client.as_ref().is_some_and(|c| c.conn == conn)
    }
}

fn bind_error(port: u16, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::AddrInUse {
        format!("Port {port} is already in use by another app; retrying")
    } else {
        format!("Cannot listen on 127.0.0.1:{port}; retrying")
    }
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    // Err = sender dropped, which also means stop.
    let _ = stop.wait_for(|v| *v).await;
}

/// Listener loop for one enabled run: bind (retrying with backoff), accept, repeat.
async fn listen(shared: Arc<Shared>, gen: u64, mut stop: watch::Receiver<bool>) {
    let cfg = shared.config.clone();
    if let Err(e) = token::load_or_create(&cfg.token_path) {
        tracing::warn!(kind = ?e.kind(), "cannot create the companion token file");
    }
    let mut attempt = 0u32;
    // Disable → enable in quick succession can find the old socket not yet closed.
    let mut quick = 5u32;
    loop {
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, cfg.port)).await {
            Ok(l) => l,
            Err(e) => {
                let delay = if quick > 0 {
                    quick -= 1;
                    Duration::from_millis(100)
                } else {
                    let msg = bind_error(cfg.port, &e);
                    tracing::warn!(port = cfg.port, kind = ?e.kind(), attempt, "companion server cannot bind");
                    shared.set_status(gen, |s| {
                        s.listening = false;
                        s.error = Some(msg);
                    });
                    let d = cfg.retry_delay(attempt);
                    attempt = attempt.saturating_add(1);
                    d
                };
                tokio::select! {
                    _ = tokio::time::sleep(delay) => continue,
                    _ = stopped(&mut stop) => return,
                }
            }
        };
        let port = listener.local_addr().map_or(cfg.port, |a| a.port());
        tracing::info!(port, "companion server listening");
        shared.set_status(gen, |s| {
            s.listening = true;
            s.error = None;
            s.port = port;
        });
        loop {
            tokio::select! {
                _ = stopped(&mut stop) => return,
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => {
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        if shared.pending.load(Ordering::Acquire) >= MAX_PENDING {
                            tracing::debug!("too many pending companion handshakes; dropping");
                            continue;
                        }
                        tauri::async_runtime::spawn(connection(shared.clone(), stream, peer, stop.clone()));
                    }
                    Err(e) => {
                        tracing::debug!(kind = ?e.kind(), "companion accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
    }
}

type Ws = WebSocketStream<TcpStream>;

async fn close_with(ws: &mut Ws, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code: CloseCode::from(code),
        reason: reason.into(),
    };
    let _ = ws.close(Some(frame)).await;
    // Wait (briefly) for the peer's close echo so the close frame is actually delivered.
    let _ = timeout(Duration::from_secs(1), async {
        while let Some(Ok(_)) = ws.next().await {}
    })
    .await;
}

struct PendingGuard<'a>(&'a AtomicUsize);

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Only `/` is served.
#[allow(clippy::result_large_err)]
fn check_path(req: &Request, resp: Response) -> Result<Response, ErrorResponse> {
    if req.uri().path() == "/" {
        Ok(resp)
    } else {
        let mut err = ErrorResponse::new(Some("not found".into()));
        *err.status_mut() = StatusCode::NOT_FOUND;
        Err(err)
    }
}

enum Auth {
    Ok {
        client_version: Option<String>,
        mode: AuthMode,
    },
    Reject(u16, &'static str),
    Gone,
}

/// The next text frame before `deadline`, skipping pings/pongs.
async fn next_text(ws: &mut Ws, deadline: Instant, late: &'static str) -> Result<String, Auth> {
    loop {
        match timeout_at(deadline, ws.next()).await {
            Err(_) => return Err(Auth::Reject(close::UNAUTHORIZED, late)),
            Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(WsMessage::Close(_)))) => {
                return Err(Auth::Gone)
            }
            Ok(Some(Ok(WsMessage::Text(t)))) => return Ok(t.as_str().to_owned()),
            Ok(Some(Ok(WsMessage::Binary(_)))) => {
                return Err(Auth::Reject(close::MALFORMED, "text frames only"))
            }
            Ok(Some(Ok(_))) => continue,
        }
    }
}

fn decode_error(e: DecodeError) -> Auth {
    match e {
        DecodeError::Malformed => Auth::Reject(close::MALFORMED, "malformed frame"),
        DecodeError::Version(_) => {
            Auth::Reject(close::UNSUPPORTED_VERSION, "unsupported protocol version")
        }
        DecodeError::Invalid { .. } => Auth::Reject(close::MALFORMED, "invalid handshake frame"),
    }
}

/// A nonce or proof: 64 hex chars, compared lowercase.
fn hex64(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    token::is_valid(&s).then_some(s)
}

async fn authenticate(shared: &Shared, ws: &mut Ws, deadline: Instant) -> Auth {
    let first = match next_text(ws, deadline, "no hello in time").await {
        Ok(t) => t,
        Err(a) => return a,
    };
    let hello = match codec::decode(&first) {
        Ok(Decoded::Message(Message::Hello(h))) => h,
        Err(e @ (DecodeError::Malformed | DecodeError::Version(_))) => return decode_error(e),
        // An unusable hello, or any other first message.
        _ => return Auth::Reject(close::UNAUTHORIZED, "hello required"),
    };
    if hello.nonce.is_none() && hello.token.is_none() {
        return Auth::Reject(close::UNAUTHORIZED, "hello required");
    }
    // Re-read per handshake so a token recreated by the desktop is picked up.
    let expected = match token::load_or_create(&shared.config.token_path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "cannot read the companion token file");
            return Auth::Reject(close::UNAUTHORIZED, "token unavailable");
        }
    };
    let client_version = hello.client_version;

    // Legacy one-step handshake (deprecated): only the client proves the token.
    let Some(nonce) = hello.nonce else {
        let given = hello.token.unwrap_or_default();
        if !token::matches(&expected, given.trim()) {
            return Auth::Reject(close::UNAUTHORIZED, "bad token");
        }
        return Auth::Ok {
            client_version,
            mode: AuthMode::Token,
        };
    };

    // Mutual handshake (protocol.md §2): prove the token first, then demand the client's proof.
    let Some(nc) = hex64(&nonce) else {
        return Auth::Reject(close::MALFORMED, "bad nonce");
    };
    let ns = match token::random_hex() {
        Ok(n) => n,
        Err(_) => return Auth::Reject(close::UNAUTHORIZED, "no randomness"),
    };
    let challenge = Message::HelloChallenge(HelloChallenge {
        nonce: ns.clone(),
        proof: token::proof(&expected, token::Role::Server, &nc, &ns),
    });
    if ws.send(WsMessage::text(challenge.encode())).await.is_err() {
        return Auth::Gone;
    }
    let second = match next_text(ws, deadline, "no hello.auth in time").await {
        Ok(t) => t,
        Err(a) => return a,
    };
    let given = match codec::decode(&second) {
        Ok(Decoded::Message(Message::HelloAuth(a))) => a.proof,
        Err(e) => return decode_error(e),
        _ => return Auth::Reject(close::UNAUTHORIZED, "hello.auth required"),
    };
    let Some(given) = hex64(&given) else {
        return Auth::Reject(close::MALFORMED, "bad proof");
    };
    let want = token::proof(&expected, token::Role::Client, &ns, &nc);
    if !token::matches(&want, &given) {
        return Auth::Reject(close::UNAUTHORIZED, "bad proof");
    }
    Auth::Ok {
        client_version,
        mode: AuthMode::Mutual,
    }
}

async fn connection(
    shared: Arc<Shared>,
    stream: TcpStream,
    peer: SocketAddr,
    mut stop: watch::Receiver<bool>,
) {
    shared.pending.fetch_add(1, Ordering::AcqRel);
    let pending = PendingGuard(&shared.pending);
    let cfg = &shared.config;
    let deadline = Instant::now() + cfg.hello_timeout;
    let ws_cfg = WebSocketConfig::default()
        .max_frame_size(Some(MAX_FRAME))
        .max_message_size(Some(MAX_FRAME));
    let accepted = timeout_at(
        deadline,
        tokio_tungstenite::accept_hdr_async_with_config(stream, check_path, Some(ws_cfg)),
    )
    .await;
    let mut ws = match accepted {
        Ok(Ok(ws)) => ws,
        Ok(Err(e)) => {
            tracing::debug!(
                port = peer.port(),
                "companion websocket handshake failed: {e}"
            );
            return;
        }
        Err(_) => return,
    };
    let (client_version, mode) = match authenticate(&shared, &mut ws, deadline).await {
        Auth::Ok {
            client_version,
            mode,
        } => (client_version, mode),
        Auth::Reject(code, reason) => {
            tracing::info!(code, reason, "companion client rejected");
            close_with(&mut ws, code, reason).await;
            return;
        }
        Auth::Gone => return,
    };
    drop(pending);

    let ok = Message::HelloOk(HelloOk {
        server: "kotilo-deck".into(),
        server_version: cfg.server_version.clone(),
        protocol: codec::PROTOCOL_VERSION,
        auth: Some(mode),
    });
    if ws.send(WsMessage::text(ok.encode())).await.is_err() {
        return;
    }

    // Register, replacing any previous client.
    let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
    let mut fx = Effects::default();
    let conn = {
        let mut st = shared.lock();
        let conn = st.next_id;
        st.next_id += 1;
        if let Some(old) = st.client.replace(Client { conn, tx }) {
            let _ = old.tx.send(Out::Close(close::REPLACED, "replaced"));
        }
        fx.snapshot = clear_snapshot(&mut st);
        st.status.connected = true;
        st.status.client = client_version.clone();
        fx.status = Some(st.status.clone());
        conn
    };
    tracing::info!(conn, version = ?client_version, auth = ?mode, "companion client connected");
    shared.fire(fx);

    let pomodoro = Message::PomodoroState(shared.host.pomodoro_state());
    let alive = ws.send(WsMessage::text(pomodoro.encode())).await.is_ok();
    if alive {
        session(&shared, &mut ws, conn, &mut rx, &mut stop).await;
    }

    // Unregister unless a newer client already took over.
    let mut fx = Effects::default();
    {
        let mut st = shared.lock();
        if st.client.as_ref().is_some_and(|c| c.conn == conn) {
            st.client = None;
            st.status.connected = false;
            st.status.client = None;
            fx.status = Some(st.status.clone());
            fx.snapshot = clear_snapshot(&mut st);
        }
    }
    tracing::info!(conn, "companion client disconnected");
    shared.fire(fx);
}

async fn session(
    shared: &Shared,
    ws: &mut Ws,
    conn: u64,
    rx: &mut mpsc::UnboundedReceiver<Out>,
    stop: &mut watch::Receiver<bool>,
) {
    let cfg = &shared.config;
    let mut ping = tokio::time::interval_at(Instant::now() + cfg.ping_interval, cfg.ping_interval);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut missed = 0u32;
    loop {
        tokio::select! {
            frame = ws.next() => match frame {
                Some(Ok(WsMessage::Text(t))) => handle(shared, conn, t.as_str()),
                Some(Ok(WsMessage::Pong(_))) => missed = 0,
                Some(Ok(WsMessage::Binary(_))) => tracing::debug!(conn, "ignoring binary companion frame"),
                Some(Ok(WsMessage::Close(_))) | None => return,
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    tracing::info!(conn, "companion connection error: {e}");
                    return;
                }
            },
            out = rx.recv() => match out {
                Some(Out::Text(s)) => {
                    if ws.send(WsMessage::text(s)).await.is_err() {
                        return;
                    }
                }
                Some(Out::Close(code, reason)) => {
                    close_with(ws, code, reason).await;
                    return;
                }
                None => return,
            },
            _ = ping.tick() => {
                if missed >= cfg.max_missed_pongs {
                    tracing::info!(conn, missed, "companion client missed pongs; dropping");
                    return;
                }
                missed += 1;
                if ws.send(WsMessage::Ping(Default::default())).await.is_err() {
                    return;
                }
            },
            _ = stopped(stop) => {
                // Deliver what was queued before the stop (e.g. a last approval.respond).
                while let Ok(Out::Text(s)) = rx.try_recv() {
                    if ws.send(WsMessage::text(s)).await.is_err() {
                        return;
                    }
                }
                close_with(ws, close::GOING_AWAY, "deck shutting down").await;
                return;
            }
        }
    }
}

/// One text frame from the authenticated client.
fn handle(shared: &Shared, conn: u64, text: &str) {
    let msg = match codec::decode(text) {
        Ok(Decoded::Message(m)) => m,
        Ok(Decoded::Unknown(kind)) => {
            tracing::debug!(conn, kind, "ignoring unknown companion message type");
            return;
        }
        Err(e) => {
            tracing::warn!(conn, "ignoring companion message: {e}");
            return;
        }
    };
    if !shared.is_current(conn) {
        return;
    }
    let kind = msg.type_name();
    tracing::debug!(conn, kind, "companion message");
    match msg {
        Message::PomodoroSuggest(s) => shared.host.suggest(&s.reason, s.minutes),
        Message::WorkspaceSwitch(w) => {
            if crate::workspace::valid_id(&w.slug) {
                shared.host.workspace_switch(&w.slug);
            } else {
                tracing::debug!(conn, "ignoring workspace.switch with an invalid slug");
            }
        }
        Message::AgentsState(_)
        | Message::ApprovalRequested(_)
        | Message::ApprovalResolved(_)
        | Message::UsageToday(_) => update_snapshot(shared, conn, msg),
        Message::Hello(_)
        | Message::HelloChallenge(_)
        | Message::HelloAuth(_)
        | Message::HelloOk(_)
        | Message::ApprovalRespond(_)
        | Message::PomodoroState(_)
        | Message::TodoAdded(_) => {
            tracing::debug!(conn, kind, "ignoring message not meant for the deck");
        }
    }
}

fn update_snapshot(shared: &Shared, conn: u64, msg: Message) {
    let mut announce: Option<(Approval, Option<Agent>)> = None;
    let snapshot = {
        let mut guard = shared.lock();
        let st = &mut *guard;
        let snap = &mut st.snapshot;
        match msg {
            Message::AgentsState(s) => {
                snap.agents = s.agents;
                snap.approvals = s.approvals;
            }
            Message::ApprovalRequested(a) => {
                tracing::info!(conn, approval = a.id, "approval requested");
                match snap.approvals.iter_mut().find(|x| x.id == a.id) {
                    Some(x) => *x = a.clone(),
                    None => snap.approvals.push(a.clone()),
                }
                if st.notified.insert(a.id.clone()) {
                    let agent = snap.agents.iter().find(|g| g.id == a.agent_id).cloned();
                    announce = Some((a, agent));
                }
            }
            Message::ApprovalResolved(r) => {
                tracing::info!(conn, approval = r.id, decision = ?r.decision, "approval resolved");
                snap.approvals.retain(|a| a.id != r.id);
            }
            Message::UsageToday(u) => snap.usage = Some(u),
            _ => return,
        }
        snap.updated_at = Some(now_ms());
        snap.clone()
    };
    shared.host.snapshot_changed(&snapshot);
    if let Some((a, agent)) = announce {
        shared.host.approval_requested(&a, agent.as_ref());
    }
}
