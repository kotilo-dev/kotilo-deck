//! Text the backend shows itself (tray menu, OS notifications) in English and Turkish.
//!
//! The UI (`src/lib/i18n`) picks its language from `navigator.language` once per page
//! load; there is no language setting. The backend mirrors that with the OS UI language
//! (`Platform::ui_language`, the same source the webview uses), read once per process:
//! primary subtag `tr` → Turkish, anything else (or an error) → English.

use std::sync::OnceLock;

use crate::platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Tr,
}

impl Lang {
    /// "tr", "tr-TR", "TR_tr" → Turkish; everything else English (same rule as the UI).
    pub fn from_tag(tag: &str) -> Self {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("tr") {
            Lang::Tr
        } else {
            Lang::En
        }
    }
}

/// The backend's language for this process.
pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| match platform::current().ui_language() {
        Ok(tag) => {
            let lang = Lang::from_tag(&tag);
            tracing::debug!(tag, ?lang, "backend text language");
            lang
        }
        Err(e) => {
            tracing::debug!("UI language unknown, using English: {e}");
            Lang::En
        }
    })
}

/// Fixed strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    TrayShowHide,
    TrayWorkspaces,
    TrayNoWorkspaces,
    TrayMonitor,
    TraySettings,
    TrayQuit,
    FocusComplete,
    BreakOver,
    Reminder,
    AnAgent,
}

/// Strings with values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg<'a> {
    /// Pomodoro length, e.g. "25 min".
    Minutes(u32),
    /// A break is next; `started` when auto-start already began it.
    Break {
        long: bool,
        started: bool,
        len: &'a str,
    },
    /// Focus session `session` is next; `len` only when auto-start began it.
    Focus {
        session: u32,
        len: Option<&'a str>,
    },
    /// Todo reminder body, `when` already formatted.
    Due(&'a str),
    MoreReminders(usize),
    NeedsApproval {
        who: &'a str,
        summary: &'a str,
    },
}

impl Lang {
    pub fn text(self, t: Text) -> &'static str {
        use Text::*;
        match (self, t) {
            (Lang::En, TrayShowHide) => "Show / Hide",
            (Lang::Tr, TrayShowHide) => "Göster / Gizle",
            (Lang::En, TrayWorkspaces) => "Workspaces",
            (Lang::Tr, TrayWorkspaces) => "Çalışma alanları",
            (Lang::En, TrayNoWorkspaces) => "(none)",
            (Lang::Tr, TrayNoWorkspaces) => "(yok)",
            (Lang::En, TrayMonitor) => "Monitor",
            (Lang::Tr, TrayMonitor) => "Monitör",
            (Lang::En, TraySettings) => "Settings…",
            (Lang::Tr, TraySettings) => "Ayarlar…",
            (Lang::En, TrayQuit) => "Quit",
            (Lang::Tr, TrayQuit) => "Çık",
            (Lang::En, FocusComplete) => "Focus session complete",
            (Lang::Tr, FocusComplete) => "Odak oturumu tamamlandı",
            (Lang::En, BreakOver) => "Break over",
            (Lang::Tr, BreakOver) => "Mola bitti",
            (Lang::En, Reminder) => "Reminder",
            (Lang::Tr, Reminder) => "Hatırlatma",
            (Lang::En, AnAgent) => "An agent",
            (Lang::Tr, AnAgent) => "Bir ajan",
        }
    }

    pub fn msg(self, m: Msg<'_>) -> String {
        match (self, m) {
            (Lang::En, Msg::Minutes(n)) => format!("{n} min"),
            (Lang::Tr, Msg::Minutes(n)) => format!("{n} dk"),
            (Lang::En, Msg::Break { long, started, len }) => {
                let kind = if long { "long" } else { "short" };
                if started {
                    let kind = if long { "Long" } else { "Short" };
                    format!("{kind} break started ({len})")
                } else {
                    format!("Time for a {kind} break ({len})")
                }
            }
            (Lang::Tr, Msg::Break { long, started, len }) => {
                let kind = if long { "Uzun" } else { "Kısa" };
                if started {
                    format!("{kind} mola başladı ({len})")
                } else {
                    format!("{kind} mola zamanı ({len})")
                }
            }
            (Lang::En, Msg::Focus { session, len: None }) => {
                format!("Ready for focus session {session}")
            }
            (
                Lang::En,
                Msg::Focus {
                    session,
                    len: Some(len),
                },
            ) => format!("Focus session {session} started ({len})"),
            (Lang::Tr, Msg::Focus { session, len: None }) => {
                format!("{session}. odak oturumuna hazır")
            }
            (
                Lang::Tr,
                Msg::Focus {
                    session,
                    len: Some(len),
                },
            ) => format!("{session}. odak oturumu başladı ({len})"),
            (Lang::En, Msg::Due(when)) => format!("Due {when}"),
            (Lang::Tr, Msg::Due(when)) => format!("Bitiş: {when}"),
            (Lang::En, Msg::MoreReminders(n)) => format!("{n} more reminders"),
            (Lang::Tr, Msg::MoreReminders(n)) => format!("{n} hatırlatma daha"),
            (Lang::En, Msg::NeedsApproval { who, summary }) => {
                format!("{who} needs approval: {summary}")
            }
            (Lang::Tr, Msg::NeedsApproval { who, summary }) => {
                format!("{who} onay bekliyor: {summary}")
            }
        }
    }

    /// Short weekday (0 = Monday) and month (0 = January) names for reminder dates.
    pub fn weekday_month(self, weekday: usize, month: usize) -> (&'static str, &'static str) {
        const EN_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        const TR_DAYS: [&str; 7] = ["Pzt", "Sal", "Çar", "Per", "Cum", "Cmt", "Paz"];
        const EN_MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        const TR_MONTHS: [&str; 12] = [
            "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
        ];
        let (days, months) = match self {
            Lang::En => (EN_DAYS, EN_MONTHS),
            Lang::Tr => (TR_DAYS, TR_MONTHS),
        };
        (days[weekday % 7], months[month % 12])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_map_like_the_ui() {
        for tr in ["tr", "tr-TR", "TR", "tr_TR"] {
            assert_eq!(Lang::from_tag(tr), Lang::Tr, "{tr}");
        }
        for en in ["en-US", "de-DE", "", "trk", "C"] {
            assert_eq!(Lang::from_tag(en), Lang::En, "{en}");
        }
    }

    #[test]
    fn every_text_is_translated() {
        use Text::*;
        for t in [
            TrayShowHide,
            TrayWorkspaces,
            TrayNoWorkspaces,
            TrayMonitor,
            TraySettings,
            TrayQuit,
            FocusComplete,
            BreakOver,
            Reminder,
            AnAgent,
        ] {
            let (en, tr) = (Lang::En.text(t), Lang::Tr.text(t));
            assert!(!en.is_empty() && !tr.is_empty(), "{t:?}");
            if t != TrayMonitor {
                assert_ne!(en, tr, "{t:?}");
            }
        }
        assert_eq!(Lang::En.text(TraySettings), "Settings…");
        assert_eq!(Lang::Tr.text(TrayQuit), "Çık");
    }

    #[test]
    fn messages_keep_the_english_wording_and_translate() {
        let len = "5 min";
        let b = |long, started| Msg::Break { long, started, len };
        assert_eq!(
            Lang::En.msg(b(false, false)),
            "Time for a short break (5 min)"
        );
        assert_eq!(
            Lang::En.msg(b(true, false)),
            "Time for a long break (5 min)"
        );
        assert_eq!(Lang::En.msg(b(false, true)), "Short break started (5 min)");
        assert_eq!(Lang::En.msg(b(true, true)), "Long break started (5 min)");
        assert_eq!(Lang::Tr.msg(b(true, true)), "Uzun mola başladı (5 min)");
        let f = |len| Msg::Focus { session: 3, len };
        assert_eq!(Lang::En.msg(f(None)), "Ready for focus session 3");
        assert_eq!(
            Lang::En.msg(f(Some("25 min"))),
            "Focus session 3 started (25 min)"
        );
        assert_eq!(Lang::Tr.msg(f(None)), "3. odak oturumuna hazır");
        assert_eq!(Lang::Tr.msg(Msg::Minutes(25)), "25 dk");
        assert_eq!(Lang::Tr.msg(Msg::Due("14:30")), "Bitiş: 14:30");
        assert_eq!(Lang::En.msg(Msg::MoreReminders(4)), "4 more reminders");
        assert_eq!(
            Lang::Tr.msg(Msg::NeedsApproval {
                who: "repo · bot",
                summary: "rm -rf"
            }),
            "repo · bot onay bekliyor: rm -rf"
        );
        assert_eq!(Lang::Tr.weekday_month(0, 10), ("Pzt", "Kas"));
        assert_eq!(Lang::En.weekday_month(6, 0), ("Sun", "Jan"));
    }
}
