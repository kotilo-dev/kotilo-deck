//! Integration tests: a real LAN server on 127.0.0.1 (ephemeral or freshly picked ports)
//! with a temp token file and a fake [`Backend`], driven by raw HTTP and WebSocket clients.

use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use super::allowlist::Topic;
use super::assets::Asset;
use super::server::{Backend, BoxFuture, Config, InvokeError, Server};
use super::LanStatus;
use crate::plugins::{PluginFile, PLUGIN_CSP};

const WAIT: Duration = Duration::from_secs(5);
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const INDEX: &str = "<!doctype html><html><head><script>boot()</script></head><body></body></html>";

struct Fake {
    status: mpsc::UnboundedSender<LanStatus>,
    wallpapers: PathBuf,
    plugin_calls: Mutex<Vec<(String, String)>>,
    invoked: Mutex<Vec<(String, Map<String, Value>)>>,
    subs: Mutex<Vec<(Topic, bool)>>,
}

impl Backend for Fake {
    fn status_changed(&self, status: &LanStatus) {
        let _ = self.status.send(status.clone());
    }

    fn asset(&self, key: &str) -> Option<Asset> {
        match key {
            "index.html" => Some(Asset {
                bytes: INDEX.as_bytes().to_vec(),
                mime: "text/html".into(),
            }),
            "settings.html" => Some(Asset {
                bytes: b"<html>settings</html>".to_vec(),
                mime: "text/html".into(),
            }),
            "_app/immutable/a.js" => Some(Asset {
                bytes: b"export {}".to_vec(),
                mime: "text/javascript".into(),
            }),
            // Like Tauri's resolver: unknown keys get the SPA shell.
            _ => Some(Asset {
                bytes: INDEX.as_bytes().to_vec(),
                mime: "text/html".into(),
            }),
        }
    }

    fn wallpapers_dir(&self) -> PathBuf {
        self.wallpapers.clone()
    }

    fn plugin_file(&self, folder: &str, path: &str) -> Option<PluginFile> {
        self.plugin_calls
            .lock()
            .unwrap()
            .push((folder.into(), path.into()));
        (folder == "clock" && path == "index.html").then(|| PluginFile {
            bytes: b"<p>plugin</p>".to_vec(),
            mime: "text/html; charset=utf-8",
        })
    }

    fn invoke(
        &self,
        command: String,
        args: Map<String, Value>,
    ) -> BoxFuture<Result<Value, InvokeError>> {
        self.invoked
            .lock()
            .unwrap()
            .push((command.clone(), args.clone()));
        Box::pin(async move {
            match command.as_str() {
                "todo_delete" => Err(InvokeError::Failed("no such todo".into())),
                "todo_add" => Err(InvokeError::BadArgs("invalid argument `text`".into())),
                _ => Ok(json!({ "command": command, "args": args })),
            }
        })
    }

    fn subscription(&self, topic: Topic, on: bool) -> BoxFuture<()> {
        self.subs.lock().unwrap().push((topic, on));
        Box::pin(async {})
    }
}

struct Harness {
    server: Server,
    fake: Arc<Fake>,
    status: mpsc::UnboundedReceiver<LanStatus>,
    _dir: tempfile::TempDir,
}

fn no_addrs() -> Vec<Ipv4Addr> {
    vec![Ipv4Addr::new(192, 168, 1, 20)]
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let token_path = dir.path().join("companion.token");
    std::fs::write(&token_path, TOKEN).unwrap();
    let wallpapers = dir.path().join("wallpapers");
    std::fs::create_dir_all(&wallpapers).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let fake = Arc::new(Fake {
        status: tx,
        wallpapers,
        plugin_calls: Mutex::default(),
        invoked: Mutex::default(),
        subs: Mutex::default(),
    });
    let mut cfg = Config::new(token_path);
    cfg.bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
    cfg.retry_base = Duration::from_millis(200);
    cfg.retry_max = Duration::from_millis(400);
    cfg.addrs = no_addrs;
    Harness {
        server: Server::new(cfg, fake.clone()),
        fake,
        status: rx,
        _dir: dir,
    }
}

async fn wait_status(h: &mut Harness, pred: impl Fn(&LanStatus) -> bool) -> LanStatus {
    let s = h.server.status();
    if pred(&s) {
        return s;
    }
    tokio::time::timeout(WAIT, async {
        loop {
            let s = h.status.recv().await.expect("status channel");
            if pred(&s) {
                return s;
            }
        }
    })
    .await
    .expect("timed out waiting for status")
}

/// Starts on an ephemeral port; returns the address.
async fn started() -> (Harness, SocketAddr) {
    let mut h = harness();
    h.server.apply(true, 0);
    wait_status(&mut h, |s| s.listening).await;
    let addr = h.server.local_addr().expect("bound");
    (h, addr)
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Raw HTTP/1.1 (no client-side path normalisation, so traversal attempts arrive as sent).
async fn http(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Reply {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = format!("{method} {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    s.write_all(req.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tokio::time::timeout(WAIT, s.read_to_end(&mut raw))
        .await
        .expect("response timeout")
        .unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("header end");
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

fn cookie() -> String {
    format!("kotilo_deck_token={TOKEN}")
}

async fn get(addr: SocketAddr, target: &str) -> Reply {
    http(addr, "GET", target, &[("Authorization", &bearer())], "").await
}

async fn post(addr: SocketAddr, command: &str, body: &str, extra: &[(&str, &str)]) -> Reply {
    let auth = bearer();
    let mut headers = vec![
        ("Authorization", auth.as_str()),
        ("Content-Type", "application/json"),
    ];
    headers.extend_from_slice(extra);
    http(
        addr,
        "POST",
        &format!("/api/invoke/{command}"),
        &headers,
        body,
    )
    .await
}

type Ws = WebSocketStream<TcpStream>;

/// Connects and reads the mandatory first `lan://hello` frame; returns the socket id.
async fn ws_connect(addr: SocketAddr, auth: Option<&str>) -> Result<(Ws, String), u16> {
    let mut req = format!("ws://{addr}/api/events")
        .into_client_request()
        .unwrap();
    if let Some(c) = auth {
        req.headers_mut().insert("Cookie", c.parse().unwrap());
    }
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut ws = match tokio_tungstenite::client_async(req, stream).await {
        Ok((ws, _)) => ws,
        Err(tokio_tungstenite::tungstenite::Error::Http(r)) => return Err(r.status().as_u16()),
        Err(e) => panic!("ws error: {e}"),
    };
    let hello = next_text(&mut ws).await;
    assert_eq!(hello["event"], "lan://hello", "{hello}");
    let id = hello["payload"]["socketId"]
        .as_str()
        .expect("socketId")
        .to_string();
    Ok((ws, id))
}

async fn next_text(ws: &mut Ws) -> Value {
    loop {
        let m = tokio::time::timeout(WAIT, ws.next())
            .await
            .expect("ws timeout")
            .expect("ws closed")
            .expect("ws error");
        if let WsMessage::Text(t) = m {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

async fn eventually(mut f: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition never became true"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ── auth ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn auth_cookie_bearer_and_401() {
    let (h, addr) = started().await;

    let r = http(addr, "GET", "/api/hello", &[], "").await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json(), json!({ "error": "unauthorized" }));
    let r = http(addr, "GET", "/", &[], "").await;
    assert_eq!(r.status, 401);
    assert!(r.header("access-control-allow-origin").is_none());

    let r = http(addr, "GET", "/?token=nope", &[], "").await;
    assert_eq!(r.status, 401);
    assert!(r.header("set-cookie").is_none());

    // Sign-in: cookie + 303 to "/" without the token.
    let r = http(addr, "GET", &format!("/?token={TOKEN}"), &[], "").await;
    assert_eq!(r.status, 303);
    assert_eq!(r.header("location"), Some("/"));
    let c = r.header("set-cookie").unwrap();
    assert!(c.starts_with(&cookie()));
    assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    // The cookie opens the SPA with a strict CSP that allows its inline bootstrap.
    let r = http(addr, "GET", "/", &[("Cookie", &cookie())], "").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, INDEX.as_bytes());
    let csp = r.header("content-security-policy").unwrap();
    assert!(csp.contains("'sha256-") && csp.contains("connect-src 'self' ws:"));
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("cache-control"), Some("no-cache"));

    // Bearer works everywhere; hello marks remote mode.
    let r = get(addr, "/api/hello").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json()["app"], "kotilo-deck");
    assert_eq!(r.json()["remote"], true);
    assert!(r.json()["version"].is_string());
    assert_eq!(r.header("cache-control"), Some("no-store"));
    let r = http(
        addr,
        "GET",
        "/api/hello",
        &[("Authorization", "Bearer wrong")],
        "",
    )
    .await;
    assert_eq!(r.status, 401);

    // Prerender paths and static files.
    assert_eq!(get(addr, "/settings").await.body, b"<html>settings</html>");
    let r = get(addr, "/_app/immutable/a.js").await;
    assert_eq!(r.status, 200);
    assert!(r.header("cache-control").unwrap().contains("immutable"));
    assert_eq!(get(addr, "/missing.js").await.status, 404);
    assert_eq!(get(addr, "/some/route").await.body, INDEX.as_bytes());
    assert_eq!(
        http(addr, "DELETE", "/", &[("Authorization", &bearer())], "")
            .await
            .status,
        405
    );
    drop(h);
}

#[tokio::test]
async fn ten_failures_per_minute_get_429() {
    let (_h, addr) = started().await;
    for _ in 0..10 {
        let r = http(
            addr,
            "GET",
            "/api/hello",
            &[("Authorization", "Bearer bad")],
            "",
        )
        .await;
        assert_eq!(r.status, 401);
    }
    // Locked out, even with the right token now.
    assert_eq!(get(addr, "/api/hello").await.status, 429);
    assert_eq!(
        http(addr, "GET", &format!("/?token={TOKEN}"), &[], "")
            .await
            .status,
        429
    );
}

#[tokio::test]
async fn missing_credentials_never_lock_out() {
    let (_h, addr) = started().await;
    for _ in 0..15 {
        assert_eq!(http(addr, "GET", "/favicon.png", &[], "").await.status, 401);
    }
    assert_eq!(get(addr, "/api/hello").await.status, 200);
}

// ── invoke ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn allowlist_and_error_shapes() {
    let (h, addr) = started().await;
    for cmd in [
        "settings_set",
        "file_open_text",
        "lan_status",
        "update_install",
        "nope",
        "todo_",
    ] {
        let r = post(addr, cmd, "{}", &[]).await;
        assert_eq!(r.status, 403, "{cmd}");
        assert!(r.json()["error"].is_string());
    }
    assert!(h.fake.invoked.lock().unwrap().is_empty());

    let r = post(
        addr,
        "weather_get",
        r#"{"latitude":1.5,"longitude":2,"unit":"celsius"}"#,
        &[],
    )
    .await;
    assert_eq!(r.status, 200);
    assert_eq!(
        r.json(),
        json!({ "command": "weather_get", "args": { "latitude": 1.5, "longitude": 2, "unit": "celsius" } })
    );
    // Empty body = no args.
    assert_eq!(
        post(addr, "todo_list", "", &[]).await.json()["args"],
        json!({})
    );

    let r = post(addr, "todo_delete", r#"{"id":1}"#, &[]).await;
    assert_eq!(
        (r.status, r.json()),
        (500, json!({ "error": "no such todo" }))
    );
    assert_eq!(post(addr, "todo_add", "{}", &[]).await.status, 400);
    assert_eq!(post(addr, "todo_list", "[1]", &[]).await.status, 400);
    assert_eq!(post(addr, "todo_list", "{bad", &[]).await.status, 400);
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(super::server::MAX_BODY));
    assert_eq!(post(addr, "todo_list", &big, &[]).await.status, 413);
    assert_eq!(get(addr, "/api/invoke/todo_list").await.status, 405);
    assert_eq!(get(addr, "/api/nothing").await.status, 404);

    // Cross-site browser requests are refused even with a valid credential.
    let r = post(
        addr,
        "todo_list",
        "{}",
        &[("Origin", "http://evil.example")],
    )
    .await;
    assert_eq!(r.status, 403);
    let origin = format!("http://{addr}");
    assert_eq!(
        post(addr, "todo_list", "{}", &[("Origin", &origin)])
            .await
            .status,
        200
    );
}

// ── logs ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Process-wide capture (server tasks run on Tauri's runtime threads).
fn captured_logs() -> Capture {
    static CAP: OnceLock<Capture> = OnceLock::new();
    CAP.get_or_init(|| {
        let cap = Capture::default();
        let writer = cap.clone();
        let sub = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        let _ = tracing::subscriber::set_global_default(sub);
        cap
    })
    .clone()
}

#[tokio::test]
async fn token_never_in_logs() {
    let logs = captured_logs();
    let (_h, addr) = started().await;
    http(addr, "GET", &format!("/?token={TOKEN}"), &[], "").await;
    http(addr, "GET", &format!("/?token={}", &TOKEN[..63]), &[], "").await;
    http(addr, "GET", "/api/hello", &[("Cookie", &cookie())], "").await;
    http(
        addr,
        "GET",
        "/api/hello",
        &[("Authorization", "Bearer guess-1234")],
        "",
    )
    .await;
    post(addr, "settings_set", "{}", &[]).await;
    post(addr, "todo_delete", "{}", &[]).await;
    let (mut ws, id) = ws_connect(addr, Some(&cookie())).await.unwrap();
    post(addr, "system_subscribe", "", &[("X-Kotilo-Socket", &id)]).await;
    ws.close(None).await.ok();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let text = String::from_utf8_lossy(&logs.0.lock().unwrap()).to_string();
    assert!(text.contains("wrong token"), "capture works: {text}");
    assert!(!text.contains(&TOKEN[..16]), "token leaked into logs");
    assert!(
        !text.contains("guess-1234"),
        "attempted token leaked into logs"
    );
    assert!(!text.contains("?token"), "query string leaked into logs");
}

// ── files ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn wallpapers_are_confined_and_support_ranges() {
    let (h, addr) = started().await;
    let dir = h.fake.wallpapers.clone();
    std::fs::write(dir.join("sky.png"), b"0123456789").unwrap();
    std::fs::write(dir.join(".hidden.png"), b"x").unwrap();
    std::fs::write(dir.join("notes.txt"), b"x").unwrap();
    let outside = dir.parent().unwrap().join("secret.png");
    std::fs::File::create(&outside)
        .unwrap()
        .write_all(b"secret")
        .unwrap();

    let r = get(addr, "/wallpapers/sky.png").await;
    assert_eq!((r.status, r.body.as_slice()), (200, &b"0123456789"[..]));
    assert_eq!(r.header("content-type"), Some("image/png"));
    assert_eq!(r.header("accept-ranges"), Some("bytes"));
    let auth = bearer();
    let r = http(
        addr,
        "GET",
        "/wallpapers/sky.png",
        &[("Authorization", &auth), ("Range", "bytes=2-4")],
        "",
    )
    .await;
    assert_eq!((r.status, r.body.as_slice()), (206, &b"234"[..]));
    assert_eq!(r.header("content-range"), Some("bytes 2-4/10"));
    let r = http(
        addr,
        "GET",
        "/wallpapers/sky.png",
        &[("Authorization", &auth), ("Range", "bytes=20-")],
        "",
    )
    .await;
    assert_eq!(r.status, 416);

    for bad in [
        "/wallpapers/../secret.png",
        "/wallpapers/..%2fsecret.png",
        "/wallpapers/..%5csecret.png",
        "/wallpapers/%2e%2e%2fsecret.png",
        "/wallpapers/.hidden.png",
        "/wallpapers/notes.txt",
        "/wallpapers/sub/sky.png",
        "/wallpapers/C:%5cWindows%5cwin.png",
        "/wallpapers/",
    ] {
        let r = get(addr, bad).await;
        assert_eq!(r.status, 404, "{bad}");
        assert_ne!(r.body, b"secret", "{bad}");
    }
    // Unauthenticated: 401 before any file access.
    assert_eq!(
        http(addr, "GET", "/wallpapers/sky.png", &[], "")
            .await
            .status,
        401
    );
}

#[tokio::test]
async fn plugin_files_get_the_plugin_csp_and_no_traversal() {
    let (h, addr) = started().await;
    let r = get(addr, "/plugins/clock/index.html").await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, b"<p>plugin</p>");
    assert_eq!(r.header("content-security-policy"), Some(PLUGIN_CSP));
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(get(addr, "/plugins/clock/missing.js").await.status, 404);
    h.fake.plugin_calls.lock().unwrap().clear();

    for bad in [
        "/plugins/clock/../../secret",
        "/plugins/../clock/index.html",
        "/plugins/clock/%2e%2e/%2e%2e/secret",
        "/plugins/clock/..%2f..%2fsecret",
        "/plugins/clock/a%5c..%5cb",
        "/plugins/clock//index.html",
        "/plugins/clock",
        "/plugins/C:/x",
    ] {
        assert_eq!(get(addr, bad).await.status, 404, "{bad}");
    }
    assert!(
        h.fake.plugin_calls.lock().unwrap().is_empty(),
        "traversal reached plugins::read_file"
    );
}

// ── events + subscriptions ───────────────────────────────────────────────────

#[tokio::test]
async fn events_fan_out_to_every_socket() {
    let (mut h, addr) = started().await;
    assert_eq!(ws_connect(addr, None).await.err(), Some(401));
    let (mut a, id_a) = ws_connect(addr, Some(&cookie())).await.unwrap();
    let (mut b, id_b) = ws_connect(addr, Some(&cookie())).await.unwrap();
    assert_ne!(id_a, id_b);
    wait_status(&mut h, |s| s.clients == 2).await;

    h.server.publish("todo://reminder", r#"{"id":3}"#);
    h.server.publish("system://sample", r#"{"cpu":5}"#);
    for ws in [&mut a, &mut b] {
        assert_eq!(
            next_text(ws).await,
            json!({ "event": "todo://reminder", "payload": { "id": 3 } })
        );
        assert_eq!(next_text(ws).await["event"], "system://sample");
    }

    a.close(None).await.ok();
    wait_status(&mut h, |s| s.clients == 1).await;
    // Disabling closes the rest with 1001.
    h.server.apply(false, 0);
    let closed = tokio::time::timeout(WAIT, async {
        while let Some(m) = b.next().await {
            if let Ok(WsMessage::Close(f)) = m {
                return f.map(|f| u16::from(f.code));
            }
        }
        None
    })
    .await
    .unwrap();
    assert_eq!(closed, Some(1001));
    let s = wait_status(&mut h, |s| !s.listening).await;
    assert_eq!(s.clients, 0);
}

#[tokio::test]
async fn subscriptions_are_owned_by_the_socket_and_released_on_close() {
    let (h, addr) = started().await;
    let (mut ws, id) = ws_connect(addr, Some(&cookie())).await.unwrap();
    let mine = [("X-Kotilo-Socket", id.as_str())];

    for cmd in ["system_subscribe", "system_subscribe", "audio_subscribe"] {
        let r = post(addr, cmd, "", &mine).await;
        assert_eq!((r.status, r.json()), (200, Value::Null), "{cmd}");
    }
    // One backend subscription per topic, however many widgets subscribe.
    assert_eq!(h.server.totals(), [2, 1, 0]);
    assert_eq!(
        *h.fake.subs.lock().unwrap(),
        [(Topic::System, true), (Topic::Audio, true)]
    );
    post(addr, "system_unsubscribe", "", &mine).await;
    post(addr, "media_unsubscribe", "", &mine).await; // never subscribed: no-op
    assert_eq!(h.server.totals(), [1, 1, 0]);
    assert_eq!(h.fake.subs.lock().unwrap().len(), 2);
    assert!(
        h.fake.invoked.lock().unwrap().is_empty(),
        "subscriptions never reach dispatch"
    );

    ws.close(None).await.ok();
    drop(ws);
    eventually(|| h.server.totals() == [0, 0, 0]).await;
    eventually(|| h.fake.subs.lock().unwrap().len() == 4).await;
    let subs = h.fake.subs.lock().unwrap().clone();
    assert!(subs[2..].contains(&(Topic::System, false)));
    assert!(subs[2..].contains(&(Topic::Audio, false)));
    // The closed socket's id is gone.
    let r = post(addr, "system_subscribe", "", &mine).await;
    assert_eq!(r.status, 400);
}

#[tokio::test]
async fn subscribe_needs_a_known_socket() {
    let (h, addr) = started().await;
    let _ws = ws_connect(addr, Some(&cookie())).await.unwrap();
    let r = post(addr, "system_subscribe", "", &[]).await;
    assert_eq!(r.status, 400);
    assert!(r.json()["error"]
        .as_str()
        .unwrap()
        .contains("X-Kotilo-Socket"));
    let r = post(
        addr,
        "media_unsubscribe",
        "",
        &[("X-Kotilo-Socket", "nope")],
    )
    .await;
    assert_eq!(r.status, 400);
    assert!(r.json()["error"].is_string());
    assert_eq!(h.server.totals(), [0, 0, 0]);
    assert!(h.fake.subs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn closing_one_socket_keeps_the_others_subscriptions() {
    let (h, addr) = started().await;
    let (a, id_a) = ws_connect(addr, Some(&cookie())).await.unwrap();
    let (_b, id_b) = ws_connect(addr, Some(&cookie())).await.unwrap();
    post(addr, "media_subscribe", "", &[("X-Kotilo-Socket", &id_a)]).await;
    post(addr, "media_subscribe", "", &[("X-Kotilo-Socket", &id_b)]).await;
    assert_eq!(h.server.totals(), [0, 0, 2]);
    drop(a);
    eventually(|| h.server.totals() == [0, 0, 1]).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(*h.fake.subs.lock().unwrap(), [(Topic::Media, true)]);
}

#[tokio::test]
async fn disabling_releases_subscriptions_at_once() {
    let (mut h, addr) = started().await;
    let (_ws, id) = ws_connect(addr, Some(&cookie())).await.unwrap();
    post(addr, "system_subscribe", "", &[("X-Kotilo-Socket", &id)]).await;
    h.server.apply(false, 0);
    wait_status(&mut h, |s| !s.listening && s.clients == 0).await;
    assert_eq!(h.server.totals(), [0, 0, 0]);
    eventually(|| h.fake.subs.lock().unwrap().last() == Some(&(Topic::System, false))).await;
}

// ── settings_changed: enable / rebind / disable / bind errors ────────────────

fn free_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn refused(port: u16) -> bool {
    TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .is_err()
}

#[tokio::test]
async fn settings_changes_rebind_and_stop() {
    let mut h = harness();
    let (p1, p2) = (free_port(), free_port());
    assert!(!h.server.status().enabled);

    h.server.apply(true, p1);
    let s = wait_status(&mut h, |s| s.listening && s.port == p1).await;
    assert!(s.enabled && s.error.is_none());
    assert_eq!(s.urls, [format!("http://192.168.1.20:{p1}/?token={TOKEN}")]);
    assert!(s.qr_svg.as_deref().is_some_and(|q| q.contains("<svg")));
    let a1 = SocketAddr::from((Ipv4Addr::LOCALHOST, p1));
    assert_eq!(get(a1, "/api/hello").await.status, 200);

    // Same settings again: no restart.
    h.server.apply(true, p1);
    assert!(h.server.status().listening);

    h.server.apply(true, p2);
    wait_status(&mut h, |s| s.listening && s.port == p2).await;
    eventually_async(|| refused(p1)).await;
    let a2 = SocketAddr::from((Ipv4Addr::LOCALHOST, p2));
    assert_eq!(get(a2, "/api/hello").await.status, 200);

    h.server.apply(false, p2);
    let s = wait_status(&mut h, |s| !s.enabled && !s.listening).await;
    assert!(s.urls.is_empty() && s.qr_svg.is_none());
    eventually_async(|| refused(p2)).await;
}

async fn eventually_async<F: std::future::Future<Output = bool>>(mut f: impl FnMut() -> F) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !f().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition never became true"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn bind_errors_are_reported_and_retried() {
    let mut h = harness();
    let blocker = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = blocker.local_addr().unwrap().port();
    h.server.apply(true, port);
    let s = wait_status(&mut h, |s| s.error.is_some()).await;
    assert!(!s.listening);
    assert!(s.error.unwrap().contains(&port.to_string()));
    // Freed: the backoff retry picks it up.
    drop(blocker);
    let s = wait_status(&mut h, |s| s.listening).await;
    assert!(s.error.is_none());
}

/// Manual smoke test: `cargo test lan::tests::smoke -- --ignored --nocapture` starts the
/// server on 127.0.0.1:47899 and curls `/api/hello` with and without the token.
#[tokio::test]
#[ignore = "manual smoke test; needs curl"]
async fn smoke() {
    let mut h = harness();
    h.server.apply(true, 47899);
    wait_status(&mut h, |s| s.listening).await;
    let curl = |args: &[&str]| {
        let out = std::process::Command::new("curl")
            .args(["-s", "-o", "-", "-w", "\n%{http_code}"])
            .args(args)
            .output()
            .expect("curl");
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let url = "http://127.0.0.1:47899/api/hello";
    let without = curl(&[url]);
    println!("without token:\n{without}");
    let with = curl(&["-H", &format!("Authorization: Bearer {TOKEN}"), url]);
    println!("with bearer token:\n{with}");
    let signin = curl(&["-D", "-", &format!("http://127.0.0.1:47899/?token={TOKEN}")]);
    println!("sign-in (headers):\n{}", signin.replace(TOKEN, "<token>"));
    assert!(without.ends_with("401"));
    assert!(with.ends_with("200") && with.contains("\"remote\":true"));
    h.server.shutdown();
}
