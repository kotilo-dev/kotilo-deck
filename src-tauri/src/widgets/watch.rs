//! Shared plumbing for push-based widgets (audio, media): a ref-counted subscription like
//! `system::Sampler`, plus a debouncer that turns bursts of OS change signals into one
//! state read + event. Nothing here polls: threads block until a signal or stop arrives.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::platform::{self, ChangeFn, Watch};

/// Quiet time after the last signal before emitting.
pub const DEBOUNCE: Duration = Duration::from_millis(75);
/// A continuous stream of signals still emits at least this often.
pub const DEBOUNCE_MAX: Duration = Duration::from_millis(300);

/// Who holds a subscription. The deck webview's count is dropped on every page load (a
/// reload never runs widget cleanup); LAN browsers (`crate::lan`) are tracked per client
/// there and released when their events socket goes away, so a deck reload never touches
/// them (and a browser closing never touches the deck's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Deck,
    Remote,
}

#[derive(Default)]
struct Counts {
    deck: u32,
    remote: u32,
}

impl Counts {
    fn get(&mut self, owner: Owner) -> &mut u32 {
        match owner {
            Owner::Deck => &mut self.deck,
            Owner::Remote => &mut self.remote,
        }
    }

    fn total(&self) -> u32 {
        self.deck.saturating_add(self.remote)
    }
}

/// Subscriber counts + the running handle (dropping the handle stops the work). The work
/// runs while the deck and remote counts together are above zero.
pub struct Subscription<H>(Mutex<(Counts, Option<H>)>);

impl<H> Default for Subscription<H> {
    fn default() -> Self {
        Self(Mutex::new((Counts::default(), None)))
    }
}

impl<H> Subscription<H> {
    #[cfg(test)]
    pub fn subscribe(&self, start: impl FnOnce() -> Option<H>) {
        self.subscribe_as(Owner::Deck, start);
    }

    #[cfg(test)]
    pub fn unsubscribe(&self) {
        self.unsubscribe_as(Owner::Deck);
    }

    /// Adds a subscriber; starts the work on 0 → 1 (or if a previous start failed).
    pub fn subscribe_as(&self, owner: Owner, start: impl FnOnce() -> Option<H>) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let n = s.0.get(owner);
        *n = n.saturating_add(1);
        if s.1.is_none() {
            s.1 = start();
        }
    }

    /// Removes a subscriber; stops the work when no subscriber of either owner is left.
    pub fn unsubscribe_as(&self, owner: Owner) {
        let handle = {
            let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
            let n = s.0.get(owner);
            *n = n.saturating_sub(1);
            if s.0.total() == 0 {
                s.1.take()
            } else {
                None
            }
        };
        drop(handle); // outside the lock: stopping a watcher may block briefly
    }

    /// Drops every deck subscription (webview reload never runs widget cleanup); the work
    /// keeps running for remote subscribers. Returns the deck count that was dropped.
    pub fn reset(&self) -> u32 {
        let (count, handle) = {
            let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
            let count = s.0.deck;
            s.0.deck = 0;
            let handle = if s.0.remote == 0 { s.1.take() } else { None };
            (count, handle)
        };
        drop(handle);
        count
    }

    #[cfg(test)]
    pub fn count(&self) -> u32 {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).0.total()
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).1.is_some()
    }
}

enum Signal {
    Changed,
    Stop,
}

/// An OS watcher plus its debouncer thread. Dropping it stops both.
pub struct Live {
    stop: Sender<Signal>,
    _watch: Watch,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.stop.send(Signal::Stop);
    }
}

/// Collapses bursts: after a signal, waits until `quiet` passes without another one (but no
/// longer than `max` since the first), then calls `emit` once.
fn debounce_loop(rx: mpsc::Receiver<Signal>, quiet: Duration, max: Duration, emit: impl Fn()) {
    while let Ok(Signal::Changed) = rx.recv() {
        let first = Instant::now();
        loop {
            let left = max.saturating_sub(first.elapsed());
            if left.is_zero() {
                break;
            }
            match rx.recv_timeout(quiet.min(left)) {
                Ok(Signal::Changed) => continue,
                Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        emit();
    }
}

/// Starts `watch` with a debounced `emit` (run on the debouncer thread, off the async runtime).
pub fn start(
    name: &str,
    watch: impl FnOnce(ChangeFn) -> platform::Result<Watch>,
    emit: impl Fn() + Send + 'static,
) -> Option<Live> {
    let (tx, rx) = mpsc::channel::<Signal>();
    let signal = tx.clone();
    let on_change: ChangeFn = Arc::new(move || {
        let _ = signal.send(Signal::Changed);
    });
    let watch = match watch(on_change) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("{name} watcher unavailable: {e}");
            return None;
        }
    };
    let spawned = std::thread::Builder::new()
        .name(format!("{name}-debounce"))
        .spawn(move || debounce_loop(rx, DEBOUNCE, DEBOUNCE_MAX, emit));
    match spawned {
        Ok(_) => Some(Live {
            stop: tx,
            _watch: watch,
        }),
        Err(e) => {
            tracing::warn!("cannot start {name} debouncer: {e}");
            None
        }
    }
}

/// Runs a blocking platform call off the async runtime.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> platform::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Flag(Arc<AtomicUsize>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn subscription_is_ref_counted() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let starts = AtomicUsize::new(0);
        let sub = Subscription::<Flag>::default();
        let start = || {
            starts.fetch_add(1, Ordering::SeqCst);
            Some(Flag(dropped.clone()))
        };
        sub.subscribe(start);
        sub.subscribe(start);
        assert_eq!((sub.count(), starts.load(Ordering::SeqCst)), (2, 1));
        sub.unsubscribe();
        assert!(sub.is_running());
        sub.unsubscribe();
        assert!(!sub.is_running());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        sub.unsubscribe(); // extra unsubscribe never underflows
        assert_eq!(sub.count(), 0);
    }

    #[test]
    fn subscription_reset_drops_everything() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let sub = Subscription::<Flag>::default();
        for _ in 0..3 {
            sub.subscribe(|| Some(Flag(dropped.clone())));
        }
        assert_eq!(sub.reset(), 3);
        assert_eq!(sub.count(), 0);
        assert!(!sub.is_running());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        // A fresh subscribe after reset starts again.
        sub.subscribe(|| Some(Flag(dropped.clone())));
        assert!(sub.is_running());
    }

    #[test]
    fn reset_keeps_remote_subscribers() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let sub = Subscription::<Flag>::default();
        sub.subscribe(|| Some(Flag(dropped.clone())));
        sub.subscribe_as(Owner::Remote, || Some(Flag(dropped.clone())));
        assert_eq!(sub.reset(), 1);
        assert!(sub.is_running(), "a remote subscriber keeps the work alive");
        assert_eq!(sub.count(), 1);
        // A deck unsubscribe after the reset never eats the remote count.
        sub.unsubscribe();
        assert!(sub.is_running());
        sub.unsubscribe_as(Owner::Remote);
        assert!(!sub.is_running());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        // Deck subscribers survive the last remote one leaving.
        sub.subscribe(|| Some(Flag(dropped.clone())));
        sub.subscribe_as(Owner::Remote, || Some(Flag(dropped.clone())));
        sub.unsubscribe_as(Owner::Remote);
        assert!(sub.is_running());
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_start_is_retried_by_next_subscriber() {
        let sub = Subscription::<u8>::default();
        sub.subscribe(|| None);
        assert!(!sub.is_running());
        sub.subscribe(|| Some(1));
        assert!(sub.is_running());
        assert_eq!(sub.count(), 2);
    }

    #[test]
    fn debounce_collapses_bursts() {
        let (tx, rx) = mpsc::channel();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let t = std::thread::spawn(move || {
            debounce_loop(
                rx,
                Duration::from_millis(30),
                Duration::from_millis(500),
                move || {
                    h.fetch_add(1, Ordering::SeqCst);
                },
            )
        });
        for _ in 0..20 {
            tx.send(Signal::Changed).unwrap();
        }
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        tx.send(Signal::Changed).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        tx.send(Signal::Stop).unwrap();
        t.join().unwrap();
    }

    #[test]
    fn debounce_caps_continuous_streams() {
        let (tx, rx) = mpsc::channel();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let t = std::thread::spawn(move || {
            debounce_loop(
                rx,
                Duration::from_millis(50),
                Duration::from_millis(100),
                move || {
                    h.fetch_add(1, Ordering::SeqCst);
                },
            )
        });
        let begin = Instant::now();
        while begin.elapsed() < Duration::from_millis(350) {
            tx.send(Signal::Changed).unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            hits.load(Ordering::SeqCst) >= 2,
            "emits despite constant signals"
        );
        drop(tx); // disconnect also stops the loop
        t.join().unwrap();
    }

    #[test]
    fn start_reports_unsupported_watch() {
        let live = start(
            "test",
            |_| Err(platform::PlatformError::Os("nope".into())),
            || {},
        );
        assert!(live.is_none());
    }

    #[test]
    fn live_drop_stops_debouncer_and_watch() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let d = dropped.clone();
        let fired = Arc::new(AtomicUsize::new(0));
        let f = fired.clone();
        let mut trigger: Option<ChangeFn> = None;
        let live = start(
            "test",
            |on_change| {
                trigger = Some(on_change);
                Ok(Watch::new(Flag(d)))
            },
            move || {
                f.fetch_add(1, Ordering::SeqCst);
            },
        )
        .unwrap();
        let trigger = trigger.unwrap();
        trigger();
        std::thread::sleep(DEBOUNCE + Duration::from_millis(100));
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        drop(live);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        trigger(); // late OS callback after stop is harmless
        std::thread::sleep(DEBOUNCE + Duration::from_millis(50));
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }
}
