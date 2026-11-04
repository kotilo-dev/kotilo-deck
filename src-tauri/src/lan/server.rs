//! The LAN HTTP + WebSocket server (ARCHITECTURE.md "LAN mode"), independent of Tauri:
//! commands, files and status changes go through [`Backend`]; ports, token path and
//! timings come from [`Config`], so tests run a real server on an ephemeral port.
//!
//! Plain hyper (HTTP/1.1) + tokio-tungstenite on the upgraded connection. Nothing here is
//! fatal: bind failures retry with backoff (like the companion, DECISIONS 51), bad
//! requests get 4xx. Logs carry route kinds, command names and status codes only, never
//! tokens, cookies, query strings or paths.
//!
//! Subscriptions: every `/api/events` socket first receives
//! `{"event":"lan://hello","payload":{"socketId":"<id>"}}` (random, server-issued). A
//! browser's `*_subscribe` / `*_unsubscribe` must carry `X-Kotilo-Socket: <id>` and is
//! owned by that socket; closing it releases them (the transport re-sends its subscribes
//! after a reconnect). The server holds one [`Backend::subscription`] per topic while any
//! socket holds one, so the deck webview's own count (reset on its page load) never mixes
//! with browsers'.

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::io::SeekFrom;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full, Limited, StreamBody};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::allowlist::{self, Topic};
use super::assets::{self, Asset};
use super::auth::{self, Credential, Limiter};
use super::events;
use super::status;
use super::LanStatus;
use crate::companion::token;
use crate::plugins::{PluginFile, PLUGIN_CSP};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
/// Buffered for everything except wallpaper files, which are streamed.
type Body = UnsyncBoxBody<Bytes, std::io::Error>;
type Resp = Response<Body>;

/// `POST /api/invoke` body cap (a workspace file is the largest argument).
pub const MAX_BODY: usize = 1 << 20;
/// Incoming WebSocket frames are ignored; this only bounds what a client can make us buffer.
const MAX_WS_IN: usize = 64 << 10;
/// Open `/api/events` sockets at once.
pub const MAX_SOCKETS: usize = 32;
/// HTTP connections at once (a tablet opens ~6); more are dropped.
const MAX_CONNECTIONS: usize = 128;
/// A `Range` request gets at most this much per response (the client asks for the rest).
const MAX_RANGE_CHUNK: u64 = 4 << 20;
/// Read size while streaming a wallpaper.
const STREAM_CHUNK: usize = 64 << 10;
const BROADCAST_CAPACITY: usize = 256;
pub const SOCKET_HEADER: &str = "x-kotilo-socket";
/// First frame on every events socket.
pub const EVENT_HELLO: &str = "lan://hello";

/// Why an allowlisted command failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// Missing/invalid argument → 400.
    BadArgs(String),
    /// The command returned an error → 500.
    Failed(String),
}

/// Side effects of the server, implemented by the Tauri glue (and by tests).
pub trait Backend: Send + Sync + 'static {
    fn status_changed(&self, status: &LanStatus);
    /// SPA file for an asset key (`index.html`, `_app/…`); `None` if not embedded.
    fn asset(&self, key: &str) -> Option<Asset>;
    fn wallpapers_dir(&self) -> PathBuf;
    /// `plugins::read_file`.
    fn plugin_file(&self, folder: &str, path: &str) -> Option<PluginFile>;
    /// An allowlisted command other than `*_subscribe` / `*_unsubscribe`.
    fn invoke(
        &self,
        command: String,
        args: Map<String, Value>,
    ) -> BoxFuture<Result<Value, InvokeError>>;
    /// One remote subscriber more (`on`) or fewer for `topic`. Calls are serialised.
    fn subscription(&self, topic: Topic, on: bool) -> BoxFuture<()>;
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: IpAddr,
    pub token_path: PathBuf,
    pub version: String,
    pub ping_interval: Duration,
    pub max_missed_pongs: u32,
    /// First bind retry delay; doubles up to `retry_max`.
    pub retry_base: Duration,
    pub retry_max: Duration,
    /// Addresses for `LanStatus.urls`.
    pub addrs: fn() -> Vec<Ipv4Addr>,
}

impl Config {
    pub fn new(token_path: PathBuf) -> Self {
        Self {
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            token_path,
            version: env!("CARGO_PKG_VERSION").to_string(),
            ping_interval: Duration::from_secs(15),
            max_missed_pongs: 3,
            retry_base: Duration::from_secs(30),
            retry_max: Duration::from_secs(300),
            addrs: status::local_ipv4s,
        }
    }

    fn retry_delay(&self, attempt: u32) -> Duration {
        self.retry_base
            .saturating_mul(1u32 << attempt.min(16))
            .min(self.retry_max)
    }
}

struct Run {
    gen: u64,
    port: u16,
    stop: watch::Sender<bool>,
}

/// An open events socket and the subscriptions it owns.
#[derive(Default)]
struct Socket {
    subs: [u32; 3],
}

struct State {
    status: LanStatus,
    run: Option<Run>,
    token: Option<String>,
    bound: Option<SocketAddr>,
    /// Open events sockets by their opaque id.
    sockets: HashMap<String, Socket>,
    /// Remote subscribers per topic across all sockets (held as one backend subscription).
    totals: [u32; 3],
    next_id: u64,
    limiter: Limiter,
}

struct HubOp {
    topic: Topic,
    on: bool,
    done: Option<oneshot::Sender<()>>,
}

struct Shared {
    config: Config,
    backend: Arc<dyn Backend>,
    state: Mutex<State>,
    events: broadcast::Sender<Arc<str>>,
    hub: mpsc::UnboundedSender<HubOp>,
    connections: AtomicUsize,
    /// Called by `remote_subscription` right after it releases the state lock.
    #[cfg(test)]
    after_unlock: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

/// Cheap to clone; all clones drive the same server.
#[derive(Clone)]
pub struct Server {
    shared: Arc<Shared>,
}

impl Server {
    pub fn new(config: Config, backend: Arc<dyn Backend>) -> Self {
        let (events, _) = broadcast::channel(BROADCAST_CAPACITY);
        let (hub, mut hub_rx) = mpsc::unbounded_channel::<HubOp>();
        // One worker keeps subscribe/unsubscribe in order (the hubs' counts saturate at 0,
        // so a reordered pair would leak a running watcher).
        let worker_backend = backend.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(op) = hub_rx.recv().await {
                worker_backend.subscription(op.topic, op.on).await;
                if let Some(done) = op.done {
                    let _ = done.send(());
                }
            }
        });
        Self {
            shared: Arc::new(Shared {
                config,
                backend,
                state: Mutex::new(State {
                    status: LanStatus::default(),
                    run: None,
                    token: None,
                    bound: None,
                    sockets: HashMap::new(),
                    totals: [0; 3],
                    next_id: 1,
                    limiter: Limiter::default(),
                }),
                events,
                hub,
                connections: AtomicUsize::new(0),
                #[cfg(test)]
                after_unlock: Mutex::new(None),
            }),
        }
    }

    pub fn status(&self) -> LanStatus {
        self.shared.lock().status.clone()
    }

    /// Address actually bound (tests bind port 0).
    #[cfg(test)]
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.shared.lock().bound
    }

    #[cfg(test)]
    pub fn totals(&self) -> [u32; 3] {
        self.shared.lock().totals
    }

    /// Starts, stops or rebinds (port change) to match the settings.
    pub fn apply(&self, enabled: bool, port: u16) {
        let mut st = self.shared.lock();
        let before = st.status.clone();
        st.status.port = port;
        st.status.enabled = enabled;
        let running = st.run.as_ref().map(|r| r.port);
        if running == Some(port) && enabled {
            return self.shared.finish(st, before);
        }
        if running.is_some() {
            self.shared.stop_run(&mut st);
        }
        if enabled {
            let gen = st.next_id;
            st.next_id += 1;
            let (tx, rx) = watch::channel(false);
            st.run = Some(Run {
                gen,
                port,
                stop: tx,
            });
            tauri::async_runtime::spawn(listen(self.shared.clone(), gen, port, rx));
        }
        self.shared.finish(st, before);
    }

    /// Quit path: stops listening and closes every connection.
    pub fn shutdown(&self) {
        let port = self.shared.lock().status.port;
        self.apply(false, port);
    }

    /// Re-reads the interface addresses (DHCP, Wi-Fi switch) for `lan_status`.
    pub fn refresh_urls(&self) {
        let (port, token) = {
            let st = self.shared.lock();
            if !st.status.listening {
                return;
            }
            (st.status.port, st.token.clone())
        };
        let Some(token) = token else { return };
        let (urls, qr) = urls_and_qr(&self.shared.config, port, &token);
        let mut st = self.shared.lock();
        let before = st.status.clone();
        if st.status.listening && st.status.urls != urls {
            st.status.urls = urls;
            st.status.qr_svg = qr;
        }
        self.shared.finish(st, before);
    }

    /// Forwards one backend event to every open `/api/events` socket.
    pub fn publish(&self, event: &str, payload_json: &str) {
        if self.shared.events.receiver_count() == 0 {
            return;
        }
        let _ = self
            .shared
            .events
            .send(Arc::from(events::frame(event, payload_json)));
    }
}

fn urls_and_qr(cfg: &Config, port: u16, token: &str) -> (Vec<String>, Option<String>) {
    let urls = status::urls(&(cfg.addrs)(), port, token);
    let qr = urls.first().and_then(|u| status::qr_svg(u));
    (urls, qr)
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Releases the lock, then reports a status change.
    fn finish(&self, mut st: MutexGuard<'_, State>, before: LanStatus) {
        st.status.clients = u32::try_from(st.sockets.len()).unwrap_or(u32::MAX);
        let changed = (st.status != before).then(|| st.status.clone());
        drop(st);
        if let Some(s) = changed {
            self.backend.status_changed(&s);
        }
    }

    fn stop_run(&self, st: &mut State) {
        if let Some(run) = st.run.take() {
            let _ = run.stop.send(true);
        }
        st.status.listening = false;
        st.status.error = None;
        st.status.error_code = None;
        st.status.urls.clear();
        st.status.qr_svg = None;
        st.bound = None;
        st.token = None;
        // Sockets close on the stop signal; their subscriptions go now.
        st.sockets.clear();
        for t in Topic::ALL {
            if std::mem::take(&mut st.totals[t.index()]) > 0 {
                self.hub_op(t, false, None);
            }
        }
    }

    /// Queues one hub change. Callers hold the state lock so ops stay in decision order.
    fn hub_op(&self, topic: Topic, on: bool, done: Option<oneshot::Sender<()>>) {
        let _ = self.hub.send(HubOp { topic, on, done });
    }

    /// Updates the status if `gen` is still the live run.
    fn set_status(&self, gen: u64, f: impl FnOnce(&mut State)) -> bool {
        let mut st = self.lock();
        if st.run.as_ref().map(|r| r.gen) != Some(gen) {
            return false;
        }
        let before = st.status.clone();
        f(&mut st);
        self.finish(st, before);
        true
    }

    fn blocked(&self, ip: IpAddr) -> bool {
        self.lock().limiter.blocked(ip, Instant::now())
    }

    fn failed(&self, ip: IpAddr) {
        let mut st = self.lock();
        st.limiter.fail(ip, Instant::now());
        if st.limiter.blocked(ip, Instant::now()) {
            tracing::warn!(
                "LAN: too many failed token attempts from one address; blocking it for a minute"
            );
        } else {
            tracing::info!("LAN: rejected a request with a wrong token");
        }
    }

    fn token_ok(&self, given: &str) -> bool {
        auth::valid(self.lock().token.as_deref(), given)
    }

    // ── subscriptions ────────────────────────────────────────────────────────

    /// A browser's `*_subscribe` / `*_unsubscribe` for the socket named by
    /// `X-Kotilo-Socket`; returns once the hub has applied it.
    ///
    /// Every hub op (here, in [`Self::socket_closed`] and [`Self::stop_run`]) is enqueued
    /// while the state lock is held, so the hub sees ON/OFF in the same order as the
    /// `totals` 0↔1 transitions; enqueueing after the unlock let a concurrent close's OFF
    /// overtake this ON and leak a running watcher.
    async fn remote_subscription(
        &self,
        socket: Option<&str>,
        topic: Topic,
        on: bool,
    ) -> Result<(), &'static str> {
        let i = topic.index();
        let done = {
            let mut guard = self.lock();
            let st = &mut *guard;
            let id = socket.ok_or("missing X-Kotilo-Socket header")?;
            let sock = st
                .sockets
                .get_mut(id)
                .ok_or("unknown or closed events socket")?;
            let edge = if on {
                sock.subs[i] += 1;
                st.totals[i] += 1;
                st.totals[i] == 1
            } else {
                if sock.subs[i] == 0 {
                    return Ok(()); // never subscribed: no-op, like the hub's saturating count
                }
                sock.subs[i] -= 1;
                st.totals[i] -= 1;
                st.totals[i] == 0
            };
            edge.then(|| {
                let (tx, rx) = oneshot::channel();
                self.hub_op(topic, on, Some(tx));
                rx
            })
        };
        // Tests run a concurrent socket close exactly here, after the unlock.
        #[cfg(test)]
        if let Some(hook) = self.after_unlock.lock().unwrap().as_ref() {
            hook();
        }
        if let Some(rx) = done {
            let _ = rx.await;
        }
        Ok(())
    }

    /// Registers a new events socket; `None` when stopped or full.
    fn socket_opened(&self) -> Option<String> {
        let mut st = self.lock();
        if st.run.is_none() || st.sockets.len() >= MAX_SOCKETS {
            return None;
        }
        let before = st.status.clone();
        let id = new_socket_id(st.next_id);
        st.next_id += 1;
        st.sockets.insert(id.clone(), Socket::default());
        self.finish(st, before);
        Some(id)
    }

    /// Unregisters a socket and releases every subscription it owned.
    fn socket_closed(&self, id: &str) {
        let mut st = self.lock();
        let before = st.status.clone();
        // Missing = the run was stopped; everything was released already.
        if let Some(sock) = st.sockets.remove(id) {
            for t in Topic::ALL {
                let n = sock.subs[t.index()];
                if n == 0 {
                    continue;
                }
                let total = &mut st.totals[t.index()];
                *total = total.saturating_sub(n);
                if *total == 0 {
                    self.hub_op(t, false, None);
                }
            }
        }
        self.finish(st, before);
    }
}

/// Unguessable, so one tab can never release another tab's subscriptions.
fn new_socket_id(seq: u64) -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Still unique per run; only unguessability is lost.
        return format!("s{seq}");
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `(LanStatus.errorCode, English message)` for a failed bind.
fn bind_error(port: u16, e: &std::io::Error) -> (&'static str, String) {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => (
            "port-in-use",
            format!("Port {port} is already in use by another app; retrying"),
        ),
        std::io::ErrorKind::PermissionDenied => (
            "permission-denied",
            format!("Not allowed to listen on port {port}; retrying"),
        ),
        _ => (
            "bind-failed",
            format!("Cannot listen on port {port}; retrying"),
        ),
    }
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
    // Err = sender dropped, which also means stop.
    let _ = stop.wait_for(|v| *v).await;
}

/// Listener loop for one enabled run: bind (retrying with backoff), accept, repeat.
async fn listen(shared: Arc<Shared>, gen: u64, port: u16, mut stop: watch::Receiver<bool>) {
    let cfg = shared.config.clone();
    let token = match token::load_or_create(&cfg.token_path) {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!(kind = ?e.kind(), "LAN: cannot read or create the token file");
            None
        }
    };
    if !shared.set_status(gen, |st| st.token = token.clone()) {
        return;
    }
    let mut attempt = 0u32;
    // Disable → enable (or a port change) can find the old socket not yet closed.
    let mut quick = 5u32;
    let listener = loop {
        match TcpListener::bind((cfg.bind, port)).await {
            Ok(l) => break l,
            Err(e) => {
                let delay = if quick > 0 {
                    quick -= 1;
                    Duration::from_millis(100)
                } else {
                    let (code, msg) = bind_error(port, &e);
                    tracing::warn!(port, kind = ?e.kind(), attempt, "LAN server cannot bind");
                    shared.set_status(gen, |st| {
                        st.status.listening = false;
                        st.status.error = Some(msg);
                        st.status.error_code = Some(code);
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
        }
    };
    let bound = listener.local_addr().ok();
    let (urls, qr) = match &token {
        Some(t) => urls_and_qr(&cfg, port, t),
        None => (Vec::new(), None),
    };
    let live = shared.set_status(gen, |st| {
        st.bound = bound;
        st.status.listening = true;
        st.status.error = token
            .is_none()
            .then(|| "Cannot read the companion token file".to_string());
        st.status.error_code = token.is_none().then_some("token-unreadable");
        st.status.urls = urls;
        st.status.qr_svg = qr;
    });
    if !live {
        return;
    }
    tracing::info!(port, "LAN server listening");
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let Some(slot) = ConnGuard::acquire(&shared) else {
                        tracing::debug!("LAN: too many connections; dropping one");
                        continue;
                    };
                    let _ = stream.set_nodelay(true);
                    tauri::async_runtime::spawn(serve_connection(shared.clone(), slot, stream, peer.ip(), stop.clone()));
                }
                Err(e) => {
                    tracing::debug!(kind = ?e.kind(), "LAN accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            _ = stopped(&mut stop) => break,
        }
    }
    tracing::info!(port, "LAN server stopped");
}

/// One of the [`MAX_CONNECTIONS`] slots; released on drop.
struct ConnGuard(Arc<Shared>);

impl ConnGuard {
    /// Check and increment in one atomic step, so a burst of accepts can't overshoot.
    fn acquire(shared: &Arc<Shared>) -> Option<Self> {
        let slots = &shared.connections;
        let mut n = slots.load(Ordering::Acquire);
        loop {
            if n >= MAX_CONNECTIONS {
                return None;
            }
            match slots.compare_exchange_weak(n, n + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(Self(shared.clone())),
                Err(now) => n = now,
            }
        }
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.connections.fetch_sub(1, Ordering::AcqRel);
    }
}

async fn serve_connection(
    shared: Arc<Shared>,
    _slot: ConnGuard,
    stream: tokio::net::TcpStream,
    ip: IpAddr,
    mut stop: watch::Receiver<bool>,
) {
    let svc_shared = shared.clone();
    let svc_stop = stop.clone();
    let service = service_fn(move |req: Request<Incoming>| {
        let shared = svc_shared.clone();
        let stop = svc_stop.clone();
        async move {
            let mut resp = route(shared, ip, req, stop).await;
            let h = resp.headers_mut();
            h.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            h.insert(
                header::REFERRER_POLICY,
                HeaderValue::from_static("no-referrer"),
            );
            h.entry(header::CACHE_CONTROL)
                .or_insert(HeaderValue::from_static("no-store"));
            Ok::<_, Infallible>(resp)
        }
    });
    let conn = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10))
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades();
    tokio::select! {
        res = conn => {
            if let Err(e) = res {
                tracing::trace!("LAN connection ended: {e}");
            }
        }
        _ = stopped(&mut stop) => {}
    }
}

// ── responses ────────────────────────────────────────────────────────────────

fn response(status: StatusCode, mime: &str, body: impl Into<Bytes>) -> Resp {
    let body = Full::new(body.into()).map_err(|never| match never {});
    let mut r = Response::new(body.boxed_unsync());
    *r.status_mut() = status;
    if let Ok(v) = HeaderValue::from_str(mime) {
        r.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    r
}

fn json_response(status: StatusCode, v: &Value) -> Resp {
    response(status, "application/json", v.to_string())
}

fn error(status: StatusCode, msg: &str) -> Resp {
    json_response(status, &json!({ "error": msg }))
}

fn unauthorized(api: bool) -> Resp {
    if api {
        error(StatusCode::UNAUTHORIZED, "unauthorized")
    } else {
        response(
            StatusCode::UNAUTHORIZED,
            "text/plain; charset=utf-8",
            "Kotilo Deck: open the link or scan the QR code from Settings > LAN on the host computer.",
        )
    }
}

/// Browsers send `Origin` on POST and WebSocket requests; it must be this server.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true; // not a browser (Bearer clients)
    };
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    match (origin.to_str().ok(), host) {
        (Some(o), Some(h)) => o.strip_prefix("http://") == Some(h),
        _ => false,
    }
}

async fn route(
    shared: Arc<Shared>,
    ip: IpAddr,
    req: Request<Incoming>,
    stop: watch::Receiver<bool>,
) -> Resp {
    let path = req.uri().path().to_string();
    let api = path.starts_with("/api/");

    // Plugin files need no credential: the plugin iframe is `sandbox="allow-scripts"` (an
    // opaque origin), so browsers withhold the SameSite=Strict cookie on its sub-resource
    // requests. They are the user's installed, read-only plugin files and the SDK, served
    // with PLUGIN_CSP and no secrets; path checks still apply, failures never count.
    if let Some(rest) = path.strip_prefix("/plugins/") {
        if !matches!(*req.method(), Method::GET | Method::HEAD) {
            return error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
        }
        return plugin(&shared, rest).await;
    }

    if shared.blocked(ip) {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many failed attempts; try again in a minute",
        );
    }

    // Sign-in: `/?token=` → cookie + redirect that drops the token from the address bar.
    if path == "/" && matches!(*req.method(), Method::GET | Method::HEAD) {
        if let Some(t) = auth::query_token(req.uri().query()) {
            if !shared.token_ok(t) {
                shared.failed(ip);
                return unauthorized(false);
            }
            let mut r = response(StatusCode::SEE_OTHER, "text/plain; charset=utf-8", "");
            let h = r.headers_mut();
            h.insert(header::LOCATION, HeaderValue::from_static("/"));
            if let Ok(v) = HeaderValue::from_str(&auth::set_cookie(t)) {
                h.insert(header::SET_COOKIE, v);
            }
            return r;
        }
    }

    match auth::credential(req.headers()) {
        Credential::None => return unauthorized(api),
        Credential::Given(t) => {
            if !shared.token_ok(t) {
                shared.failed(ip);
                return unauthorized(api);
            }
        }
    }

    let method = req.method().clone();
    if api {
        return match (method, path.as_str()) {
            (Method::GET, "/api/hello") => json_response(
                StatusCode::OK,
                &json!({ "app": "kotilo-deck", "version": shared.config.version, "remote": true }),
            ),
            (Method::GET, "/api/events") => upgrade(&shared, req, stop),
            (Method::POST, p) if p.starts_with("/api/invoke/") => invoke(&shared, req).await,
            (_, "/api/hello" | "/api/events") => {
                error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
            }
            (_, p) if p.starts_with("/api/invoke/") => {
                error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
            }
            _ => error(StatusCode::NOT_FOUND, "not found"),
        };
    }
    if !matches!(method, Method::GET | Method::HEAD) {
        return error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    }
    if let Some(rest) = path.strip_prefix("/wallpapers/") {
        return wallpaper(&shared, rest, req.headers()).await;
    }
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok());
    static_file(&shared, &path, host)
}

async fn invoke(shared: &Arc<Shared>, req: Request<Incoming>) -> Resp {
    let command = req.uri().path()["/api/invoke/".len()..].to_string();
    if !allowlist::allowed(&command) {
        tracing::debug!("LAN: denied a command that is not on the allowlist");
        return error(StatusCode::FORBIDDEN, "command not allowed over LAN");
    }
    if !same_origin(req.headers()) {
        return error(StatusCode::FORBIDDEN, "cross-origin request");
    }
    let socket = req
        .headers()
        .get(SOCKET_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = match Limited::new(req.into_body(), MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        Map::new()
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(Value::Object(m)) => m,
            Ok(Value::Null) => Map::new(),
            _ => return error(StatusCode::BAD_REQUEST, "arguments must be a JSON object"),
        }
    };
    if let Some((topic, on)) = allowlist::subscription(&command) {
        return match shared
            .remote_subscription(socket.as_deref(), topic, on)
            .await
        {
            Ok(()) => json_response(StatusCode::OK, &Value::Null),
            Err(m) => error(StatusCode::BAD_REQUEST, m),
        };
    }
    match shared.backend.invoke(command.clone(), args).await {
        Ok(v) => json_response(StatusCode::OK, &v),
        Err(InvokeError::BadArgs(m)) => error(StatusCode::BAD_REQUEST, &m),
        Err(InvokeError::Failed(m)) => {
            tracing::debug!(command = %command, "LAN command failed");
            error(StatusCode::INTERNAL_SERVER_ERROR, &m)
        }
    }
}

fn static_file(shared: &Shared, path: &str, host: Option<&str>) -> Resp {
    let Some(keys) = assets::candidates(path) else {
        return error(StatusCode::NOT_FOUND, "not found");
    };
    for key in keys {
        let found = shared
            .backend
            .asset(&key)
            .or_else(|| assets::from_build_dir(&key));
        let Some(a) = found else { continue };
        let html = a.mime.starts_with("text/html");
        // Tauri's resolver answers unknown keys with index.html; a missing script or image
        // must be a 404, not the SPA shell.
        if html && !key.ends_with(".html") {
            continue;
        }
        let csp = html.then(|| assets::page_csp(&a.bytes, host));
        let mut r = response(StatusCode::OK, &a.mime, a.bytes);
        let h = r.headers_mut();
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(assets::cache_control(&key)),
        );
        if let Some(Ok(v)) = csp.map(|c| HeaderValue::from_str(&c)) {
            h.insert(header::CONTENT_SECURITY_POLICY, v);
        }
        return r;
    }
    error(StatusCode::NOT_FOUND, "not found")
}

async fn plugin(shared: &Arc<Shared>, rest: &str) -> Resp {
    let Some(segs) = assets::safe_segments(rest).filter(|s| s.len() >= 2) else {
        return error(StatusCode::NOT_FOUND, "not found");
    };
    let backend = shared.backend.clone();
    let folder = segs[0].clone();
    let path = segs[1..].join("/");
    let file = tauri::async_runtime::spawn_blocking(move || backend.plugin_file(&folder, &path))
        .await
        .ok()
        .flatten();
    let Some(f) = file else {
        return error(StatusCode::NOT_FOUND, "not found");
    };
    let mut r = response(StatusCode::OK, f.mime, f.bytes);
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PLUGIN_CSP),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

/// A parsed single `Range: bytes=…` header.
#[derive(Debug, PartialEq, Eq)]
pub enum Range {
    Full,
    Part(u64, u64),
    Unsatisfiable,
}

/// At most [`MAX_RANGE_CHUNK`] bytes per part, whatever the client asked for.
pub fn parse_range(header: Option<&str>, len: u64) -> Range {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Range::Full;
    };
    if spec.contains(',') {
        return Range::Full; // multipart ranges: send the whole file
    }
    let Some((a, b)) = spec.split_once('-') else {
        return Range::Full;
    };
    let (a, b) = (a.trim(), b.trim());
    if len == 0 {
        return Range::Unsatisfiable;
    }
    let range = if a.is_empty() {
        // Suffix: the last N bytes.
        match b.parse::<u64>() {
            Ok(0) | Err(_) => return Range::Unsatisfiable,
            Ok(n) => (len.saturating_sub(n), len - 1),
        }
    } else {
        let Ok(start) = a.parse::<u64>() else {
            return Range::Unsatisfiable;
        };
        if start >= len {
            return Range::Unsatisfiable;
        }
        let end = if b.is_empty() {
            len - 1
        } else {
            match b.parse::<u64>() {
                Ok(e) if e >= start => e.min(len - 1),
                _ => return Range::Unsatisfiable,
            }
        };
        (start, end)
    };
    // Every range is capped (206 with a shorter Content-Range; the client asks for more).
    let (start, end) = range;
    Range::Part(start, end.min(start + MAX_RANGE_CHUNK - 1))
}

/// Streams `len` bytes of `file` from its current position in [`STREAM_CHUNK`] reads, so a
/// large wallpaper is never buffered whole.
fn file_body(file: tokio::fs::File, len: u64) -> Body {
    let chunks = futures_util::stream::unfold((file, len), |(mut f, left)| async move {
        if left == 0 {
            return None;
        }
        let want = usize::try_from(left).map_or(STREAM_CHUNK, |l| l.min(STREAM_CHUNK));
        let mut buf = vec![0u8; want];
        match f.read(&mut buf).await {
            Ok(0) => {
                let e = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "file shrank");
                Some((Err(e), (f, 0)))
            }
            Ok(n) => {
                buf.truncate(n);
                let left = left.saturating_sub(n as u64);
                Some((Ok(Frame::data(Bytes::from(buf))), (f, left)))
            }
            Err(e) => Some((Err(e), (f, 0))),
        }
    });
    StreamBody::new(chunks).boxed_unsync()
}

async fn read_wallpaper(dir: &Path, name: &str, range: Option<&str>) -> Option<Resp> {
    let path = dir.join(name);
    // Symlinks pointing outside the wallpapers folder are not served.
    let real = tokio::fs::canonicalize(&path).await.ok()?;
    let real_dir = tokio::fs::canonicalize(dir).await.ok()?;
    if !real.starts_with(&real_dir) {
        return None;
    }
    let mut f = tokio::fs::File::open(&real).await.ok()?;
    let len = f.metadata().await.ok().filter(|m| m.is_file())?.len();
    let mime = assets::mime_for(name);
    let (status, start, count) = match parse_range(range, len) {
        Range::Full => (StatusCode::OK, 0, len),
        Range::Part(start, end) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        Range::Unsatisfiable => {
            let mut r = response(StatusCode::RANGE_NOT_SATISFIABLE, "text/plain", "");
            let h = r.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&format!("bytes */{len}")) {
                h.insert(header::CONTENT_RANGE, v);
            }
            h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
            return Some(r);
        }
    };
    if start > 0 {
        f.seek(SeekFrom::Start(start)).await.ok()?;
    }
    let mut r = Response::new(file_body(f, count));
    *r.status_mut() = status;
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
    if status == StatusCode::PARTIAL_CONTENT {
        let last = start + count - 1;
        if let Ok(v) = HeaderValue::from_str(&format!("bytes {start}-{last}/{len}")) {
            h.insert(header::CONTENT_RANGE, v);
        }
    }
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Some(r)
}

async fn wallpaper(shared: &Arc<Shared>, rest: &str, headers: &HeaderMap) -> Resp {
    let name = match assets::percent_decode(rest) {
        Some(n)
            if crate::settings::valid_file_name(&n)
                && crate::files::wallpaper_kind(Path::new(&n)).is_some() =>
        {
            n
        }
        _ => return error(StatusCode::NOT_FOUND, "not found"),
    };
    let dir = shared.backend.wallpapers_dir();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    read_wallpaper(&dir, &name, range)
        .await
        .unwrap_or_else(|| error(StatusCode::NOT_FOUND, "not found"))
}

// ── /api/events ──────────────────────────────────────────────────────────────

fn upgrade(shared: &Arc<Shared>, req: Request<Incoming>, stop: watch::Receiver<bool>) -> Resp {
    let h = req.headers();
    let has = |name: header::HeaderName, needle: &str| {
        h.get_all(name).iter().any(|v| {
            v.to_str()
                .is_ok_and(|s| s.split(',').any(|p| p.trim().eq_ignore_ascii_case(needle)))
        })
    };
    if !has(header::CONNECTION, "upgrade") || !has(header::UPGRADE, "websocket") {
        return error(StatusCode::UPGRADE_REQUIRED, "WebSocket upgrade required");
    }
    if h.get(header::SEC_WEBSOCKET_VERSION)
        .map(HeaderValue::as_bytes)
        != Some(b"13")
    {
        return error(StatusCode::BAD_REQUEST, "unsupported WebSocket version");
    }
    if !same_origin(h) {
        return error(StatusCode::FORBIDDEN, "cross-origin request");
    }
    let Some(key) = h.get(header::SEC_WEBSOCKET_KEY) else {
        return error(StatusCode::BAD_REQUEST, "missing WebSocket key");
    };
    let accept = derive_accept_key(key.as_bytes());
    let Some(socket) = shared.socket_opened() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "too many open connections");
    };
    // Subscribe before answering so no event emitted after the 101 is missed.
    let rx = shared.events.subscribe();
    let task_shared = shared.clone();
    tauri::async_runtime::spawn(async move {
        let shared = task_shared;
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let cfg = WebSocketConfig::default()
                    .max_frame_size(Some(MAX_WS_IN))
                    .max_message_size(Some(MAX_WS_IN));
                let ws = WebSocketStream::from_raw_socket(
                    TokioIo::new(upgraded),
                    Role::Server,
                    Some(cfg),
                )
                .await;
                tracing::debug!("LAN events socket open");
                let hello = events::frame(EVENT_HELLO, &json!({ "socketId": socket }).to_string());
                pump(&shared, ws, hello, rx, stop).await;
                tracing::debug!("LAN events socket closed");
            }
            Err(e) => tracing::debug!("LAN WebSocket upgrade failed: {e}"),
        }
        shared.socket_closed(&socket);
    });
    let mut r = response(StatusCode::SWITCHING_PROTOCOLS, "text/plain", "");
    let h = r.headers_mut();
    h.remove(header::CONTENT_TYPE);
    h.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
    h.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    if let Ok(v) = HeaderValue::from_str(&accept) {
        h.insert(header::SEC_WEBSOCKET_ACCEPT, v);
    }
    r
}

async fn pump<S>(
    shared: &Shared,
    mut ws: WebSocketStream<S>,
    hello: String,
    mut rx: broadcast::Receiver<Arc<str>>,
    mut stop: watch::Receiver<bool>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // Always first: events queued in `rx` since the 101 follow it.
    if ws.send(WsMessage::text(hello)).await.is_err() {
        return;
    }
    let cfg = &shared.config;
    let start = tokio::time::Instant::now() + cfg.ping_interval;
    let mut ping = tokio::time::interval_at(start, cfg.ping_interval);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut missed = 0u32;
    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(text) => {
                    if ws.send(WsMessage::text(text.to_string())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::debug!(skipped = n, "LAN events socket lagging; events dropped");
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            frame = ws.next() => match frame {
                Some(Ok(WsMessage::Pong(_))) => missed = 0,
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => return,
                // Pings are answered by tungstenite; client text/binary is not used.
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if missed >= cfg.max_missed_pongs {
                    tracing::debug!(missed, "LAN events socket missed pongs; dropping");
                    return;
                }
                missed += 1;
                if ws.send(WsMessage::Ping(Default::default())).await.is_err() {
                    return;
                }
            },
            _ = stopped(&mut stop) => {
                let frame = CloseFrame {
                    code: CloseCode::Away,
                    reason: "deck LAN mode stopped".into(),
                };
                let _ = tokio::time::timeout(Duration::from_secs(1), ws.close(Some(frame))).await;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records hub ops; everything else is inert.
    #[derive(Default)]
    struct HubLog(Mutex<Vec<(Topic, bool)>>);

    impl Backend for HubLog {
        fn status_changed(&self, _: &LanStatus) {}
        fn asset(&self, _: &str) -> Option<Asset> {
            None
        }
        fn wallpapers_dir(&self) -> PathBuf {
            PathBuf::new()
        }
        fn plugin_file(&self, _: &str, _: &str) -> Option<PluginFile> {
            None
        }
        fn invoke(
            &self,
            _: String,
            _: Map<String, Value>,
        ) -> BoxFuture<Result<Value, InvokeError>> {
            Box::pin(async { Ok(Value::Null) })
        }
        fn subscription(&self, topic: Topic, on: bool) -> BoxFuture<()> {
            self.0.lock().unwrap().push((topic, on));
            Box::pin(async {})
        }
    }

    /// S1: a `*_subscribe` racing its socket's close must never leave the hub with a
    /// subscription the server no longer counts (OFF overtaking ON leaked a watcher).
    #[test]
    fn subscribe_racing_socket_close_never_leaks() {
        let log = Arc::new(HubLog::default());
        let srv = Server::new(Config::new(PathBuf::from("unused.token")), log.clone());
        let shared = srv.shared.clone();
        {
            // A live run without a listener: enough for sockets to register.
            let (stop, _) = watch::channel(false);
            shared.lock().run = Some(Run {
                gen: 0,
                port: 0,
                stop,
            });
        }
        for _ in 0..300 {
            let id = shared.socket_opened().expect("socket slot");
            let gate = Arc::new(std::sync::Barrier::new(2));
            let sub = {
                let (shared, id, gate) = (shared.clone(), id.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    let _ = tauri::async_runtime::block_on(shared.remote_subscription(
                        Some(&id),
                        Topic::System,
                        true,
                    ));
                })
            };
            let close = {
                let (shared, gate) = (shared.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    shared.socket_closed(&id);
                })
            };
            sub.join().unwrap();
            close.join().unwrap();
        }
        assert_eq!(shared.lock().totals, [0, 0, 0]);
        // One more subscribe on a fresh socket: awaiting it drains every earlier op.
        let id = shared.socket_opened().expect("socket slot");
        tauri::async_runtime::block_on(shared.remote_subscription(Some(&id), Topic::System, true))
            .unwrap();
        let ops = log.0.lock().unwrap().clone();
        // Strict ON, OFF, ON, OFF, …, ON: the hub's count is 1, like the server's.
        assert_eq!(ops.len() % 2, 1, "{ops:?}");
        for (i, (topic, on)) in ops.iter().enumerate() {
            assert_eq!(*topic, Topic::System);
            assert_eq!(*on, i % 2 == 0, "op {i} out of order: {ops:?}");
        }
        assert_eq!(shared.lock().totals, [1, 0, 0]);
    }

    /// S1, deterministic: the socket closes in the exact window between the subscribe's
    /// unlock and its await. Its OFF must still reach the hub after the ON.
    #[test]
    fn socket_close_right_after_subscribe_unlock_keeps_hub_order() {
        let log = Arc::new(HubLog::default());
        let srv = Server::new(Config::new(PathBuf::from("unused.token")), log.clone());
        let shared = srv.shared.clone();
        let (stop, _) = watch::channel(false);
        shared.lock().run = Some(Run {
            gen: 0,
            port: 0,
            stop,
        });
        let id = shared.socket_opened().expect("socket slot");
        let weak = Arc::downgrade(&shared);
        let closing = id.clone();
        *shared.after_unlock.lock().unwrap() = Some(Box::new(move || {
            if let Some(s) = weak.upgrade() {
                s.socket_closed(&closing);
            }
        }));
        tauri::async_runtime::block_on(shared.remote_subscription(Some(&id), Topic::Media, true))
            .unwrap();
        *shared.after_unlock.lock().unwrap() = None;
        // The hub applies ops in order; a fresh subscribe drains it.
        let probe = shared.socket_opened().expect("socket slot");
        tauri::async_runtime::block_on(shared.remote_subscription(
            Some(&probe),
            Topic::System,
            true,
        ))
        .unwrap();
        assert_eq!(
            *log.0.lock().unwrap(),
            [
                (Topic::Media, true),
                (Topic::Media, false),
                (Topic::System, true)
            ]
        );
        assert_eq!(shared.lock().totals, [1, 0, 0]);
    }

    #[test]
    fn connection_slots_are_capped() {
        let srv = Server::new(
            Config::new(PathBuf::from("unused.token")),
            Arc::new(HubLog::default()),
        );
        let slots: Vec<_> = (0..MAX_CONNECTIONS)
            .map(|_| ConnGuard::acquire(&srv.shared).expect("slot"))
            .collect();
        assert!(ConnGuard::acquire(&srv.shared).is_none());
        drop(slots);
        assert_eq!(srv.shared.connections.load(Ordering::Acquire), 0);
        assert!(ConnGuard::acquire(&srv.shared).is_some());
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Range::Full);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), Range::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=90-"), 100), Range::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Range::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=0-500"), 100), Range::Part(0, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-1"), 100), Range::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Range::Full);
        let big = 10 * MAX_RANGE_CHUNK;
        assert_eq!(
            parse_range(Some("bytes=0-"), big),
            Range::Part(0, MAX_RANGE_CHUNK - 1)
        );
        // Explicit and suffix ranges are capped too.
        assert_eq!(
            parse_range(Some(&format!("bytes=5-{}", big - 1)), big),
            Range::Part(5, 5 + MAX_RANGE_CHUNK - 1)
        );
        assert_eq!(
            parse_range(Some(&format!("bytes=-{big}")), big),
            Range::Part(0, MAX_RANGE_CHUNK - 1)
        );
    }

    #[test]
    fn socket_ids_are_random_hex() {
        let (a, b) = (new_socket_id(1), new_socket_id(2));
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn origin_check() {
        let mut h = HeaderMap::new();
        assert!(same_origin(&h));
        h.insert(header::HOST, HeaderValue::from_static("10.0.0.5:47832"));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://10.0.0.5:47832"),
        );
        assert!(same_origin(&h));
        h.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://evil.example"),
        );
        assert!(!same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("null"));
        assert!(!same_origin(&h));
    }
}
