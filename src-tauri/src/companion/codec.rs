//! protocol.md v1 message types and the envelope codec `{ v, type, payload }`.
//!
//! Decoding is lenient where the protocol says so (unknown types and fields are ignored,
//! unknown agent status = idle, optional fields default) and strict elsewhere: a message
//! that fails validation is an error the caller logs (type + error position only, never
//! content) and ignores.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

pub use crate::widgets::pomodoro::PomodoroState;

pub const PROTOCOL_VERSION: u64 = 1;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// Legacy one-step handshake (deprecated): the raw token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Mutual handshake: the client's nonce (64 lowercase hex). Takes precedence over `token`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
}

/// Never prints the token.
impl std::fmt::Debug for Hello {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hello")
            .field("nonce", &self.nonce)
            .field("client", &self.client)
            .field("client_version", &self.client_version)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloOk {
    pub server: String,
    pub server_version: String,
    pub protocol: u64,
    /// How the client was authenticated. Absent from decks before 1.0.1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// Two-step HMAC handshake: both sides proved the token.
    Mutual,
    /// Legacy `hello { token }`: only the client proved it.
    Token,
}

/// Server's half of the mutual handshake: its nonce and its proof of the token.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloChallenge {
    pub nonce: String,
    pub proof: String,
}

/// Client's proof of the token, answering `hello.challenge`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloAuth {
    pub proof: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Working,
    Waiting,
}

/// Unknown values are shown as idle (protocol.md §5).
impl<'de> Deserialize<'de> for AgentStatus {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d)?.as_str() {
            "working" => AgentStatus::Working,
            "waiting" => AgentStatus::Waiting,
            _ => AgentStatus::Idle,
        })
    }
}

/// `Agent` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub repo: String,
    pub name: String,
    pub status: AgentStatus,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub since: Option<i64>,
}

/// `Approval` in `src/lib/ipc/index.ts`. Summary/detail are user content: never log them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub agent_id: String,
    pub summary: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub requested_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsState {
    pub agents: Vec<Agent>,
    #[serde(default)]
    pub approvals: Vec<Approval>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    Approve,
    Deny,
    Expired,
    /// No longer pending, outcome not known (e.g. answered in the agent's terminal).
    Unknown,
}

/// Unrecognised values count as `unknown`: the approval is resolved either way.
impl<'de> Deserialize<'de> for Resolution {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d)?.as_str() {
            "approve" => Resolution::Approve,
            "deny" => Resolution::Deny,
            "expired" => Resolution::Expired,
            _ => Resolution::Unknown,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedBy {
    Deck,
    Desktop,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalResolved {
    pub id: String,
    pub decision: Resolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<ResolvedBy>,
}

/// `Usage` in `src/lib/ipc/index.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub tokens: u64,
    pub cost_usd: f64,
    #[serde(default)]
    pub budget_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroSuggest {
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub minutes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSwitch {
    pub slug: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRespond {
    pub id: String,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoAdded {
    pub id: i64,
    pub text: String,
    #[serde(default)]
    pub due: Option<i64>,
}

/// Every v1 message, both directions.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    // Handshake
    Hello(Hello),
    HelloChallenge(HelloChallenge),
    HelloAuth(HelloAuth),
    HelloOk(HelloOk),
    // Desktop → Deck
    AgentsState(AgentsState),
    ApprovalRequested(Approval),
    ApprovalResolved(ApprovalResolved),
    UsageToday(Usage),
    PomodoroSuggest(PomodoroSuggest),
    WorkspaceSwitch(WorkspaceSwitch),
    // Deck → Desktop
    ApprovalRespond(ApprovalRespond),
    PomodoroState(PomodoroState),
    TodoAdded(TodoAdded),
}

impl Message {
    pub fn type_name(&self) -> &'static str {
        match self {
            Message::Hello(_) => "hello",
            Message::HelloChallenge(_) => "hello.challenge",
            Message::HelloAuth(_) => "hello.auth",
            Message::HelloOk(_) => "hello.ok",
            Message::AgentsState(_) => "agents.state",
            Message::ApprovalRequested(_) => "agent.approval.requested",
            Message::ApprovalResolved(_) => "agent.approval.resolved",
            Message::UsageToday(_) => "usage.today",
            Message::PomodoroSuggest(_) => "pomodoro.suggest",
            Message::WorkspaceSwitch(_) => "workspace.switch",
            Message::ApprovalRespond(_) => "approval.respond",
            Message::PomodoroState(_) => "pomodoro.state",
            Message::TodoAdded(_) => "todo.added",
        }
    }

    fn payload(&self) -> serde_json::Result<Value> {
        match self {
            Message::Hello(p) => serde_json::to_value(p),
            Message::HelloChallenge(p) => serde_json::to_value(p),
            Message::HelloAuth(p) => serde_json::to_value(p),
            Message::HelloOk(p) => serde_json::to_value(p),
            Message::AgentsState(p) => serde_json::to_value(p),
            Message::ApprovalRequested(p) => serde_json::to_value(p),
            Message::ApprovalResolved(p) => serde_json::to_value(p),
            Message::UsageToday(p) => serde_json::to_value(p),
            Message::PomodoroSuggest(p) => serde_json::to_value(p),
            Message::WorkspaceSwitch(p) => serde_json::to_value(p),
            Message::ApprovalRespond(p) => serde_json::to_value(p),
            Message::PomodoroState(p) => serde_json::to_value(p),
            Message::TodoAdded(p) => serde_json::to_value(p),
        }
    }

    /// One text frame.
    pub fn encode(&self) -> String {
        let payload = self.payload().unwrap_or_else(|_| Value::Object(Map::new()));
        serde_json::json!({ "v": PROTOCOL_VERSION, "type": self.type_name(), "payload": payload })
            .to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    Message(Message),
    /// Valid envelope, type this build doesn't know: ignore (debug log).
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// Not JSON or not an envelope (`v` integer + `type` string).
    Malformed,
    /// A `v` other than 1.
    Version(u64),
    /// Known type, payload failed validation. `reason` carries no content.
    Invalid { kind: String, reason: String },
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Malformed => write!(f, "malformed envelope"),
            DecodeError::Version(v) => write!(f, "unsupported protocol version {v}"),
            DecodeError::Invalid { kind, reason } => write!(f, "invalid {kind}: {reason}"),
        }
    }
}

/// serde error without its message (which can quote payload content).
fn redact(e: &serde_json::Error) -> String {
    format!("{:?} error at column {}", e.classify(), e.column())
}

fn parse<T: for<'de> Deserialize<'de>>(kind: &str, payload: Value) -> Result<T, DecodeError> {
    serde_json::from_value(payload).map_err(|e| DecodeError::Invalid {
        kind: kind.to_string(),
        reason: format!("{:?} error", e.classify()),
    })
}

fn invalid(kind: &str, reason: &str) -> DecodeError {
    DecodeError::Invalid {
        kind: kind.to_string(),
        reason: reason.to_string(),
    }
}

pub fn decode(text: &str) -> Result<Decoded, DecodeError> {
    let doc: Value = serde_json::from_str(text).map_err(|e| {
        tracing::debug!(reason = %redact(&e), "companion frame is not JSON");
        DecodeError::Malformed
    })?;
    let Value::Object(mut env) = doc else {
        return Err(DecodeError::Malformed);
    };
    let v = env
        .get("v")
        .and_then(Value::as_u64)
        .ok_or(DecodeError::Malformed)?;
    let kind = match env.remove("type") {
        Some(Value::String(s)) => s,
        _ => return Err(DecodeError::Malformed),
    };
    if v != PROTOCOL_VERSION {
        return Err(DecodeError::Version(v));
    }
    let payload = match env.remove("payload") {
        None | Some(Value::Null) => Value::Object(Map::new()),
        Some(p @ Value::Object(_)) => p,
        Some(_) => return Err(invalid(&kind, "payload is not an object")),
    };
    let k = kind.as_str();
    let msg = match k {
        "hello" => Message::Hello(parse(k, payload)?),
        "hello.challenge" => Message::HelloChallenge(parse(k, payload)?),
        "hello.auth" => Message::HelloAuth(parse(k, payload)?),
        "hello.ok" => Message::HelloOk(parse(k, payload)?),
        "agents.state" => Message::AgentsState(parse(k, payload)?),
        "agent.approval.requested" => Message::ApprovalRequested(parse(k, payload)?),
        "agent.approval.resolved" => Message::ApprovalResolved(parse(k, payload)?),
        "usage.today" => {
            let u: Usage = parse(k, payload)?;
            let ok = |x: f64| x.is_finite() && x >= 0.0;
            if !ok(u.cost_usd) || u.budget_usd.is_some_and(|b| !ok(b)) {
                return Err(invalid(k, "negative amount"));
            }
            Message::UsageToday(u)
        }
        "pomodoro.suggest" => {
            let s: PomodoroSuggest = parse(k, payload)?;
            if s.minutes.is_some_and(|m| !(1..=180).contains(&m)) {
                return Err(invalid(k, "minutes out of range"));
            }
            Message::PomodoroSuggest(s)
        }
        "workspace.switch" => Message::WorkspaceSwitch(parse(k, payload)?),
        "approval.respond" => Message::ApprovalRespond(parse(k, payload)?),
        "pomodoro.state" => Message::PomodoroState(parse(k, payload)?),
        "todo.added" => Message::TodoAdded(parse(k, payload)?),
        _ => return Ok(Decoded::Unknown(kind)),
    };
    Ok(Decoded::Message(msg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::pomodoro::Phase;
    use serde_json::json;

    fn round_trip(m: Message) {
        let text = m.encode();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["type"], m.type_name());
        assert_eq!(decode(&text), Ok(Decoded::Message(m)));
    }

    fn approval() -> Approval {
        Approval {
            id: "ap-17".into(),
            agent_id: "a2".into(),
            summary: "Run cargo build".into(),
            detail: Some("cargo build --release".into()),
            requested_at: 1_793_733_100_000,
        }
    }

    #[test]
    fn every_message_round_trips() {
        round_trip(Message::Hello(Hello {
            token: Some("ab".repeat(32)),
            nonce: None,
            client: Some("kotilo-desktop".into()),
            client_version: Some("1.4.0".into()),
        }));
        round_trip(Message::HelloOk(HelloOk {
            server: "kotilo-deck".into(),
            server_version: "0.0.1".into(),
            protocol: 1,
            auth: Some(AuthMode::Mutual),
        }));
        round_trip(Message::Hello(Hello {
            token: None,
            nonce: Some("a0".repeat(32)),
            client: Some("kotilo-desktop".into()),
            client_version: None,
        }));
        round_trip(Message::HelloChallenge(HelloChallenge {
            nonce: "c0".repeat(32),
            proof: "7a".repeat(32),
        }));
        round_trip(Message::HelloAuth(HelloAuth {
            proof: "19".repeat(32),
        }));
        round_trip(Message::AgentsState(AgentsState {
            agents: vec![
                Agent {
                    id: "a1".into(),
                    repo: "kotilo-deck".into(),
                    name: "claude".into(),
                    status: AgentStatus::Working,
                    task: Some("Phase 3 widgets".into()),
                    since: Some(1_793_733_000_000),
                },
                Agent {
                    id: "a2".into(),
                    repo: "shell".into(),
                    name: "claude".into(),
                    status: AgentStatus::Waiting,
                    task: None,
                    since: None,
                },
            ],
            approvals: vec![approval()],
        }));
        round_trip(Message::ApprovalRequested(approval()));
        round_trip(Message::ApprovalResolved(ApprovalResolved {
            id: "ap-17".into(),
            decision: Resolution::Expired,
            by: Some(ResolvedBy::Agent),
        }));
        round_trip(Message::UsageToday(Usage {
            tokens: 1_840_000,
            cost_usd: 4.12,
            budget_usd: Some(10.0),
        }));
        round_trip(Message::PomodoroSuggest(PomodoroSuggest {
            reason: "Agent started a long task".into(),
            minutes: Some(25),
        }));
        round_trip(Message::WorkspaceSwitch(WorkspaceSwitch {
            slug: "agents".into(),
        }));
        round_trip(Message::ApprovalRespond(ApprovalRespond {
            id: "ap-17".into(),
            decision: Decision::Deny,
        }));
        round_trip(Message::PomodoroState(PomodoroState {
            phase: Phase::Focus,
            running: true,
            ends_at: Some(1_793_734_500_000),
            remaining: 1500,
            duration: 1500,
            session: 2,
            sessions_per_cycle: 4,
        }));
        round_trip(Message::TodoAdded(TodoAdded {
            id: 42,
            text: "deploy".into(),
            due: None,
        }));
    }

    #[test]
    fn protocol_md_examples_decode() {
        let agents = r#"{ "v": 1, "type": "agents.state", "payload": {
          "agents": [
            { "id": "a1", "repo": "kotilo-deck", "name": "claude", "status": "working", "task": "Phase 3 widgets", "since": 1793733000000 },
            { "id": "a2", "repo": "shell", "name": "claude", "status": "waiting", "task": "Run cargo build?", "since": 1793733100000 }
          ],
          "approvals": [
            { "id": "ap-17", "agentId": "a2", "summary": "Run cargo build", "detail": "cargo build --release", "requestedAt": 1793733100000 }
          ]
        } }"#;
        let Ok(Decoded::Message(Message::AgentsState(s))) = decode(agents) else {
            panic!("agents.state");
        };
        assert_eq!(s.agents.len(), 2);
        assert_eq!(s.approvals[0], approval());
        let usage = r#"{ "v": 1, "type": "usage.today", "payload": { "tokens": 1840000, "costUsd": 4.12 } }"#;
        assert!(matches!(
            decode(usage),
            Ok(Decoded::Message(Message::UsageToday(Usage {
                budget_usd: None,
                ..
            })))
        ));
        let resolved = r#"{ "v": 1, "type": "agent.approval.resolved", "payload": { "id": "ap-17", "decision": "approve", "by": "deck" } }"#;
        assert!(decode(resolved).is_ok());
        // Deck → desktop wire shape of pomodoro.state is exactly the protocol example.
        let state = Message::PomodoroState(PomodoroState {
            phase: Phase::ShortBreak,
            running: false,
            ends_at: None,
            remaining: 120,
            duration: 300,
            session: 3,
            sessions_per_cycle: 4,
        });
        let v: Value = serde_json::from_str(&state.encode()).unwrap();
        assert_eq!(
            v["payload"],
            json!({"phase": "short-break", "running": false, "endsAt": null, "remaining": 120,
                   "duration": 300, "session": 3, "sessionsPerCycle": 4})
        );
    }

    #[test]
    fn lenient_where_the_protocol_says_so() {
        // Unknown type.
        assert_eq!(
            decode(r#"{"v":1,"type":"agents.future","payload":{}}"#),
            Ok(Decoded::Unknown("agents.future".into()))
        );
        // Unknown fields, unknown status, missing optionals, missing payload on a type
        // whose fields are all optional.
        let text = r#"{"v":1,"type":"agents.state","extra":1,"payload":{"agents":[
            {"id":"a","repo":"r","name":"n","status":"sleeping","color":"x"}],"later":true}}"#;
        let Ok(Decoded::Message(Message::AgentsState(s))) = decode(text) else {
            panic!("agents.state");
        };
        assert_eq!(s.agents[0].status, AgentStatus::Idle);
        assert_eq!(s.agents[0].task, None);
        assert!(s.approvals.is_empty());
        assert_eq!(
            decode(r#"{"v":1,"type":"pomodoro.suggest"}"#),
            Ok(Decoded::Message(Message::PomodoroSuggest(
                PomodoroSuggest {
                    reason: String::new(),
                    minutes: None
                }
            )))
        );
        let resolved =
            r#"{"v":1,"type":"agent.approval.resolved","payload":{"id":"x","decision":"deny"}}"#;
        assert!(decode(resolved).is_ok());
    }

    #[test]
    fn invalid_messages_are_errors_without_content() {
        assert_eq!(decode("not json"), Err(DecodeError::Malformed));
        assert_eq!(decode("[1]"), Err(DecodeError::Malformed));
        assert_eq!(decode(r#"{"type":"hello"}"#), Err(DecodeError::Malformed));
        assert_eq!(
            decode(r#"{"v":"1","type":"hello"}"#),
            Err(DecodeError::Malformed)
        );
        assert_eq!(decode(r#"{"v":1,"type":5}"#), Err(DecodeError::Malformed));
        assert_eq!(
            decode(r#"{"v":2,"type":"hello","payload":{}}"#),
            Err(DecodeError::Version(2))
        );
        for bad in [
            r#"{"v":1,"type":"hello","payload":{"token":5}}"#,
            r#"{"v":1,"type":"hello.challenge","payload":{"nonce":"x"}}"#,
            r#"{"v":1,"type":"hello.auth","payload":{}}"#,
            r#"{"v":1,"type":"agents.state","payload":{"agents":"secret approval text"}}"#,
            r#"{"v":1,"type":"agents.state","payload":[]}"#,
            r#"{"v":1,"type":"agent.approval.requested","payload":{"id":"x"}}"#,
            r#"{"v":1,"type":"agent.approval.resolved","payload":{"id":"x","decision":7}}"#,
            r#"{"v":1,"type":"usage.today","payload":{"tokens":-1,"costUsd":1}}"#,
            r#"{"v":1,"type":"usage.today","payload":{"tokens":1,"costUsd":-1}}"#,
            r#"{"v":1,"type":"pomodoro.suggest","payload":{"minutes":0}}"#,
            r#"{"v":1,"type":"pomodoro.suggest","payload":{"minutes":999}}"#,
            r#"{"v":1,"type":"workspace.switch","payload":{}}"#,
        ] {
            let e = decode(bad).unwrap_err();
            assert!(matches!(e, DecodeError::Invalid { .. }), "{bad}");
            assert!(!e.to_string().contains("secret"), "{e}");
        }
    }

    #[test]
    fn hello_debug_hides_the_token() {
        let h = Hello {
            token: Some("deadbeef".repeat(8)),
            nonce: None,
            client: None,
            client_version: Some("1".into()),
        };
        assert!(!format!("{h:?}").contains("deadbeef"));
    }

    #[test]
    fn unknown_and_unrecognised_decisions_resolve_as_unknown() {
        for d in ["unknown", "timeout", "something-new"] {
            let text = format!(
                r#"{{"v":1,"type":"agent.approval.resolved","payload":{{"id":"claude-code:s1:1793733100000","decision":"{d}","by":"agent"}}}}"#
            );
            let Ok(Decoded::Message(Message::ApprovalResolved(r))) = decode(&text) else {
                panic!("{d}");
            };
            assert_eq!(r.decision, Resolution::Unknown);
        }
    }

    #[test]
    fn hello_ok_without_auth_still_decodes() {
        let text = r#"{"v":1,"type":"hello.ok","payload":{"server":"kotilo-deck","serverVersion":"1.0.0","protocol":1}}"#;
        let Ok(Decoded::Message(Message::HelloOk(ok))) = decode(text) else {
            panic!();
        };
        assert_eq!(ok.auth, None);
    }
}
