//! Token auth for LAN requests (ARCHITECTURE.md "LAN mode"): the companion token arrives
//! once as `/?token=` and then lives in an HttpOnly cookie; `Authorization: Bearer` works
//! too. Failed attempts are rate-limited per IP. Nothing here logs a credential.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use hyper::header::{HeaderMap, AUTHORIZATION, COOKIE};

use crate::companion::token;

pub const COOKIE_NAME: &str = "kotilo_deck_token";
/// 400 days, the browser maximum: a tablet stays signed in until the token changes.
const COOKIE_MAX_AGE: u64 = 400 * 24 * 60 * 60;

pub const MAX_FAILURES: usize = 10;
pub const FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// Distinct IPs tracked at once; stale ones are pruned first.
const MAX_TRACKED: usize = 1024;

/// What the request presented. `None` = no credential at all (not counted as a failure, so
/// a browser opening the bare URL never locks itself out).
#[derive(Debug, PartialEq, Eq)]
pub enum Credential<'a> {
    None,
    Given(&'a str),
}

/// Cookie first, then `Authorization: Bearer`.
pub fn credential(headers: &HeaderMap) -> Credential<'_> {
    for value in headers.get_all(COOKIE) {
        let Ok(v) = value.to_str() else { continue };
        for pair in v.split(';') {
            if let Some((name, val)) = pair.trim().split_once('=') {
                if name.trim() == COOKIE_NAME {
                    return Credential::Given(val.trim());
                }
            }
        }
    }
    if let Some(v) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some((scheme, t)) = v.trim().split_once(' ') {
            if scheme.eq_ignore_ascii_case("bearer") {
                return Credential::Given(t.trim());
            }
        }
    }
    Credential::None
}

/// The `token` value of a query string, if present.
pub fn query_token(query: Option<&str>) -> Option<&str> {
    query?.split('&').find_map(|kv| kv.strip_prefix("token="))
}

pub fn valid(expected: Option<&str>, given: &str) -> bool {
    expected.is_some_and(|e| token::matches(e, given))
}

pub fn set_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={COOKIE_MAX_AGE}")
}

/// Sliding-window failure counter per IP.
#[derive(Default)]
pub struct Limiter {
    failures: HashMap<IpAddr, VecDeque<Instant>>,
}

impl Limiter {
    fn prune(q: &mut VecDeque<Instant>, now: Instant) {
        while q
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= FAILURE_WINDOW)
        {
            q.pop_front();
        }
    }

    /// True while `ip` has [`MAX_FAILURES`] failures inside the last minute.
    pub fn blocked(&mut self, ip: IpAddr, now: Instant) -> bool {
        let Some(q) = self.failures.get_mut(&ip) else {
            return false;
        };
        Self::prune(q, now);
        if q.is_empty() {
            self.failures.remove(&ip);
            return false;
        }
        q.len() >= MAX_FAILURES
    }

    pub fn fail(&mut self, ip: IpAddr, now: Instant) {
        if self.failures.len() >= MAX_TRACKED && !self.failures.contains_key(&ip) {
            self.failures.retain(|_, q| {
                Self::prune(q, now);
                !q.is_empty()
            });
            if self.failures.len() >= MAX_TRACKED {
                // Still full of live entries: forget the oldest-failing IP.
                if let Some(oldest) = self
                    .failures
                    .iter()
                    .min_by_key(|(_, q)| q.back().copied())
                    .map(|(ip, _)| *ip)
                {
                    self.failures.remove(&oldest);
                }
            }
        }
        let q = self.failures.entry(ip).or_default();
        Self::prune(q, now);
        q.push_back(now);
        while q.len() > MAX_FAILURES {
            q.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;
    use std::net::Ipv4Addr;

    #[test]
    fn credential_from_cookie_or_bearer() {
        let mut h = HeaderMap::new();
        assert_eq!(credential(&h), Credential::None);
        h.insert(
            COOKIE,
            HeaderValue::from_static("a=1; kotilo_deck_token=abc ; b=2"),
        );
        assert_eq!(credential(&h), Credential::Given("abc"));
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_static("bearer xyz"));
        assert_eq!(credential(&h), Credential::Given("xyz"));
        h.insert(AUTHORIZATION, HeaderValue::from_static("Basic xyz"));
        assert_eq!(credential(&h), Credential::None);
    }

    #[test]
    fn query_token_is_found() {
        assert_eq!(query_token(Some("a=1&token=t")), Some("t"));
        assert_eq!(query_token(Some("xtoken=t")), None);
        assert_eq!(query_token(None), None);
    }

    #[test]
    fn cookie_is_http_only_and_strict() {
        let c = set_cookie("t");
        assert!(c.starts_with("kotilo_deck_token=t;"));
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Path=/"));
    }

    #[test]
    fn limiter_blocks_after_ten_failures_for_a_minute() {
        let mut l = Limiter::default();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let t0 = Instant::now();
        for i in 0..MAX_FAILURES {
            assert!(!l.blocked(ip, t0), "{i}");
            l.fail(ip, t0);
        }
        assert!(l.blocked(ip, t0));
        assert!(!l.blocked(other, t0));
        assert!(l.blocked(ip, t0 + Duration::from_secs(59)));
        assert!(!l.blocked(ip, t0 + FAILURE_WINDOW));
    }

    #[test]
    fn limiter_memory_is_bounded() {
        let mut l = Limiter::default();
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED as u32 + 50) {
            l.fail(IpAddr::V4(Ipv4Addr::from(i)), t0);
        }
        assert!(l.failures.len() <= MAX_TRACKED);
    }
}
