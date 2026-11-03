//! Weather widget backend: Open-Meteo (no API key).
//!
//! Reports are cached per (lat, lon rounded to 2 decimals, unit): 15 min fresh in memory,
//! and on disk under `~/.kotilo/deck/cache/weather/` so a restart or an offline deck still
//! shows the last report, flagged `stale: true` with its original `fetchedAt`.
//! Coordinates and queries are never logged (PII).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

const GEOCODE_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const TIMEOUT: Duration = Duration::from_secs(10);
const FRESH_MS: i64 = 15 * 60 * 1000;
const HOURLY_COUNT: usize = 6;
const DAILY_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    C,
    F,
}

impl Unit {
    fn as_str(self) -> &'static str {
        match self {
            Unit::C => "c",
            Unit::F => "f",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub country: String,
    pub admin1: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Current {
    pub temp: f64,
    pub code: u32,
    pub is_day: bool,
    pub high: f64,
    pub low: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hour {
    pub time: String,
    pub temp: f64,
    pub code: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Day {
    pub date: String,
    pub max: f64,
    pub min: f64,
    pub code: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherReport {
    pub fetched_at: i64,
    pub stale: bool,
    pub unit: Unit,
    pub current: Current,
    pub hourly: Vec<Hour>,
    pub daily: Vec<Day>,
}

// ── Parsing (pure; unit-tested with fixtures) ───────────────────────────────

#[derive(Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoResult>,
}

#[derive(Deserialize)]
struct GeoResult {
    name: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    admin1: Option<String>,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    timezone: Option<String>,
}

pub fn parse_geocode(body: &str) -> Result<Vec<Place>, String> {
    let r: GeoResponse =
        serde_json::from_str(body).map_err(|e| format!("unexpected geocoding response: {e}"))?;
    Ok(r.results
        .into_iter()
        .map(|g| Place {
            name: g.name,
            country: g.country.unwrap_or_default(),
            admin1: g.admin1.filter(|s| !s.is_empty()),
            latitude: g.latitude,
            longitude: g.longitude,
            timezone: g.timezone.unwrap_or_default(),
        })
        .collect())
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn code(v: Option<&Value>) -> Option<u32> {
    v.and_then(Value::as_u64)
        .and_then(|c| u32::try_from(c).ok())
}

fn arr<'a>(v: &'a Value, block: &str, key: &str) -> &'a [Value] {
    v[block][key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub fn parse_forecast(body: &str, unit: Unit, fetched_at: i64) -> Result<WeatherReport, String> {
    let bad = |what: &str| format!("unexpected forecast response: {what}");
    let v: Value = serde_json::from_str(body).map_err(|e| bad(&e.to_string()))?;

    let cur = &v["current"];
    let now = cur["time"].as_str().ok_or_else(|| bad("current.time"))?;
    let temp = num(cur.get("temperature_2m")).ok_or_else(|| bad("current.temperature_2m"))?;
    let cur_code = code(cur.get("weather_code")).ok_or_else(|| bad("current.weather_code"))?;
    let is_day = cur["is_day"].as_u64().map(|d| d != 0).unwrap_or(true);

    let times = arr(&v, "hourly", "time");
    let temps = arr(&v, "hourly", "temperature_2m");
    let codes = arr(&v, "hourly", "weather_code");
    // Times are local ISO strings of equal width (timezone=auto), so they compare lexically.
    let hourly: Vec<Hour> = times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let time = t.as_str()?;
            (time > now).then_some(())?;
            Some(Hour {
                time: time.to_string(),
                temp: num(temps.get(i))?,
                code: code(codes.get(i))?,
            })
        })
        .take(HOURLY_COUNT)
        .collect();

    let dates = arr(&v, "daily", "time");
    let maxs = arr(&v, "daily", "temperature_2m_max");
    let mins = arr(&v, "daily", "temperature_2m_min");
    let dcodes = arr(&v, "daily", "weather_code");
    let daily: Vec<Day> = dates
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            Some(Day {
                date: d.as_str()?.to_string(),
                max: num(maxs.get(i))?,
                min: num(mins.get(i))?,
                code: code(dcodes.get(i))?,
            })
        })
        .take(DAILY_COUNT)
        .collect();

    let today = daily.first().ok_or_else(|| bad("daily"))?;
    Ok(WeatherReport {
        fetched_at,
        stale: false,
        unit,
        current: Current {
            temp,
            code: cur_code,
            is_day,
            high: today.max,
            low: today.min,
        },
        hourly,
        daily,
    })
}

pub fn cache_key(latitude: f64, longitude: f64, unit: Unit) -> String {
    // `+ 0.0` turns -0.0 into 0.0 so both round to the same key.
    let r = |x: f64| (x * 100.0).round() / 100.0 + 0.0;
    format!("{:.2}_{:.2}_{}", r(latitude), r(longitude), unit.as_str())
}

fn is_fresh(report: &WeatherReport, now: i64) -> bool {
    now - report.fetched_at < FRESH_MS && report.fetched_at <= now
}

// ── State + commands ────────────────────────────────────────────────────────

pub struct Weather {
    client: Result<Client, String>,
    cache_dir: PathBuf,
    mem: Mutex<HashMap<String, WeatherReport>>,
}

impl Weather {
    pub fn new(cache_dir: PathBuf) -> Self {
        let client = Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("KotiloDeck/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| {
                tracing::warn!("cannot build HTTP client: {e}");
                format!("HTTP client unavailable: {e}")
            });
        Self {
            client,
            cache_dir,
            mem: Mutex::new(HashMap::new()),
        }
    }

    fn client(&self) -> Result<&Client, String> {
        self.client.as_ref().map_err(Clone::clone)
    }

    async fn get_text(&self, url: Url) -> Result<String, String> {
        let resp = self
            .client()?
            .get(url)
            .send()
            .await
            .map_err(|e| net_error("request", &e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("weather service returned HTTP {}", status.as_u16()));
        }
        resp.text().await.map_err(|e| net_error("read", &e))
    }

    fn cached(&self, key: &str) -> Option<WeatherReport> {
        let mut mem = self.mem.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(r) = mem.get(key) {
            return Some(r.clone());
        }
        let path = self.cache_dir.join(format!("{key}.json"));
        let report: WeatherReport = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())?;
        mem.insert(key.to_string(), report.clone());
        Some(report)
    }

    fn store(&self, key: &str, report: &WeatherReport) {
        self.mem
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), report.clone());
        let write = || -> std::io::Result<()> {
            fs::create_dir_all(&self.cache_dir)?;
            let path = self.cache_dir.join(format!("{key}.json"));
            crate::workspace::write_atomic(&path, &serde_json::to_vec(report).unwrap_or_default())
        };
        if let Err(e) = write() {
            tracing::warn!("cannot write weather cache: {e}");
        }
    }

    pub async fn geocode(&self, query: &str) -> Result<Vec<Place>, String> {
        let q = query.trim();
        if q.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let url = Url::parse_with_params(
            GEOCODE_URL,
            &[("name", q), ("count", "5"), ("format", "json")],
        )
        .map_err(|e| e.to_string())?;
        parse_geocode(&self.get_text(url).await?)
    }

    pub async fn get(
        &self,
        latitude: f64,
        longitude: f64,
        unit: Unit,
    ) -> Result<WeatherReport, String> {
        if !(latitude.is_finite() && (-90.0..=90.0).contains(&latitude))
            || !(longitude.is_finite() && (-180.0..=180.0).contains(&longitude))
        {
            return Err("coordinates out of range".into());
        }
        let key = cache_key(latitude, longitude, unit);
        let now = chrono::Utc::now().timestamp_millis();
        let cached = self.cached(&key);
        if let Some(r) = cached.as_ref().filter(|r| is_fresh(r, now)) {
            return Ok(WeatherReport {
                stale: false,
                ..r.clone()
            });
        }

        let (lat, lon) = (latitude.to_string(), longitude.to_string());
        let mut params = vec![
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("current", "temperature_2m,weather_code,is_day"),
            ("hourly", "temperature_2m,weather_code"),
            (
                "daily",
                "temperature_2m_max,temperature_2m_min,weather_code",
            ),
            ("timezone", "auto"),
            ("forecast_days", "3"),
        ];
        if unit == Unit::F {
            params.push(("temperature_unit", "fahrenheit"));
        }
        let url = Url::parse_with_params(FORECAST_URL, &params).map_err(|e| e.to_string())?;

        let fetched = match self.get_text(url).await {
            Ok(body) => parse_forecast(&body, unit, now),
            Err(e) => Err(e),
        };
        match fetched {
            Ok(report) => {
                self.store(&key, &report);
                Ok(report)
            }
            Err(e) => match cached {
                Some(r) => {
                    tracing::info!("weather fetch failed, serving stale cache: {e}");
                    Ok(WeatherReport { stale: true, ..r })
                }
                None => {
                    tracing::warn!("weather fetch failed with no cache: {e}");
                    Err(e)
                }
            },
        }
    }
}

/// Error text without the URL (it contains the user's coordinates / search query).
fn net_error(stage: &str, e: &reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else if e.is_decode() || e.is_body() {
        "bad response body"
    } else {
        "network error"
    };
    format!("weather {stage} failed: {kind}")
}

#[tauri::command]
pub async fn weather_geocode(
    weather: State<'_, Weather>,
    query: String,
) -> Result<Vec<Place>, String> {
    weather.geocode(&query).await
}

#[tauri::command]
pub async fn weather_get(
    weather: State<'_, Weather>,
    latitude: f64,
    longitude: f64,
    unit: Unit,
) -> Result<WeatherReport, String> {
    weather.get(latitude, longitude, unit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORECAST: &str = include_str!("fixtures/forecast.json");
    const GEOCODE: &str = include_str!("fixtures/geocode.json");

    #[test]
    fn parses_forecast_fixture() {
        let r = parse_forecast(FORECAST, Unit::C, 1_000).unwrap();
        assert_eq!(r.fetched_at, 1_000);
        assert!(!r.stale);
        assert_eq!(r.current.temp, 14.2);
        assert_eq!(r.current.code, 3);
        assert!(r.current.is_day);
        assert_eq!((r.current.high, r.current.low), (17.1, 9.4));
        // current.time is 14:15 → next six full hours start at 15:00.
        assert_eq!(r.hourly.len(), 6);
        assert_eq!(r.hourly[0].time, "2026-11-03T15:00");
        assert_eq!(r.hourly[5].time, "2026-11-03T20:00");
        assert_eq!(r.hourly[0].code, 61);
        assert_eq!(r.daily.len(), 3);
        assert_eq!(r.daily[2].date, "2026-11-05");
        assert_eq!(r.daily[1].code, 80);

        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["fetchedAt"], 1_000);
        assert_eq!(json["unit"], "c");
        assert_eq!(json["current"]["isDay"], true);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_forecast("{}", Unit::F, 0).is_err());
        assert!(parse_forecast("nope", Unit::F, 0).is_err());
    }

    #[test]
    fn parses_geocode_fixture() {
        let places = parse_geocode(GEOCODE).unwrap();
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].name, "Istanbul");
        assert_eq!(places[0].country, "Türkiye");
        assert_eq!(places[0].admin1.as_deref(), Some("Istanbul"));
        assert_eq!(places[1].admin1, None);
        assert_eq!(places[1].country, "");
        assert!(parse_geocode(r#"{"generationtime_ms":0.1}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn cache_key_rounds() {
        assert_eq!(cache_key(41.0082, 28.9784, Unit::C), "41.01_28.98_c");
        assert_eq!(cache_key(41.0149, 28.9751, Unit::C), "41.01_28.98_c");
        assert_eq!(cache_key(-0.001, 0.0, Unit::F), "0.00_0.00_f");
    }

    #[test]
    fn unit_deserializes_lowercase() {
        assert_eq!(serde_json::from_str::<Unit>("\"f\"").unwrap(), Unit::F);
        assert!(serde_json::from_str::<Unit>("\"k\"").is_err());
    }

    #[tokio::test]
    async fn offline_serves_stale_disk_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut old = parse_forecast(FORECAST, Unit::C, 5).unwrap();
        old.fetched_at = 5; // long expired
        let w = Weather::new(dir.path().to_path_buf());
        w.store(&cache_key(1.0, 2.0, Unit::C), &old);

        // Fresh instance = "after restart"; unreachable upstream simulated by a dead client.
        let w2 = Weather {
            client: Err("offline".into()),
            ..Weather::new(dir.path().to_path_buf())
        };
        let r = w2.get(1.0, 2.0, Unit::C).await.unwrap();
        assert!(r.stale);
        assert_eq!(r.fetched_at, 5);
        assert!(w2.get(3.0, 4.0, Unit::C).await.is_err());
    }

    #[test]
    fn freshness() {
        let mut r = parse_forecast(FORECAST, Unit::C, 0).unwrap();
        r.fetched_at = 1_000_000;
        assert!(is_fresh(&r, 1_000_000 + FRESH_MS - 1));
        assert!(!is_fresh(&r, 1_000_000 + FRESH_MS));
        assert!(!is_fresh(&r, 0)); // clock went backwards
    }
}
