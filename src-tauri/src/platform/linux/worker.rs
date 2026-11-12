//! A lazily started, long-lived job thread (the Linux counterpart of `windows/com.rs`): the
//! media backend keeps its D-Bus connection, HTTP runtime and caches in thread-locals there.
//! Idle it blocks on its queue (no CPU).

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use crate::platform::{PlatformError, Result};

/// A caller never waits longer than this for one job (a hung media player must not hang a
/// command forever; the job itself keeps running on the abandoned worker).
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

type Job = Box<dyn FnOnce() + Send>;

pub struct Worker {
    name: &'static str,
    tx: Mutex<Option<Sender<Job>>>,
}

impl Worker {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            tx: Mutex::new(None),
        }
    }

    fn spawn(&self) -> Result<Sender<Job>> {
        let (tx, rx) = mpsc::channel::<Job>();
        let name = self.name;
        std::thread::Builder::new()
            .name(self.name.into())
            .spawn(move || {
                // A panicking job must not kill the thread: jobs already queued behind it would be
                // dropped with the receiver ("call aborted"), and the thread-local state would be lost.
                // The panicking job's own caller still gets an error (its result sender is dropped).
                for job in rx {
                    if panic::catch_unwind(AssertUnwindSafe(job)).is_err() {
                        tracing::warn!(worker = name, "job panicked; worker keeps running");
                    }
                }
            })
            .map_err(|e| PlatformError::Os(format!("cannot start {}: {e}", self.name)))?;
        Ok(tx)
    }

    /// Runs `f` on the worker and waits for its result.
    pub fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (rtx, rrx) = mpsc::sync_channel::<Result<T>>(1);
        let mut job: Option<Job> = Some(Box::new(move || {
            let _ = rtx.send(f());
        }));
        {
            let mut slot = self.tx.lock().unwrap_or_else(|p| p.into_inner());
            // Two attempts: if the worker died (a panicking job), respawn it once.
            for _ in 0..2 {
                if slot.is_none() {
                    *slot = Some(self.spawn()?);
                }
                let Some(tx) = slot.as_ref() else { break };
                match job.take().map(|j| tx.send(j)) {
                    Some(Ok(())) => break,
                    Some(Err(mpsc::SendError(j))) => {
                        job = Some(j);
                        *slot = None;
                    }
                    None => break,
                }
            }
        }
        if job.is_some() {
            return Err(PlatformError::Os(format!("{} unavailable", self.name)));
        }
        match rrx.recv_timeout(JOB_TIMEOUT) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = None;
                tracing::warn!(worker = self.name, "call timed out; respawning worker");
                Err(PlatformError::Os(format!("{} call timed out", self.name)))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(PlatformError::Os(format!("{} call aborted", self.name)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_WORKER: Worker = Worker::new("linux-worker-test");

    #[test]
    fn worker_runs_jobs_and_survives_errors_and_panics() {
        assert_eq!(TEST_WORKER.run(|| Ok(2 + 2)).unwrap(), 4);
        assert!(TEST_WORKER
            .run(|| Err::<(), _>(PlatformError::Os("x".into())))
            .is_err());
        let a = TEST_WORKER.run(|| Ok(std::thread::current().id())).unwrap();
        let b = TEST_WORKER.run(|| Ok(std::thread::current().id())).unwrap();
        assert_eq!(a, b, "jobs share one dedicated thread");
        assert_ne!(a, std::thread::current().id());
        assert!(TEST_WORKER
            .run(|| -> Result<()> { panic!("job panicked") })
            .is_err());
        let c = TEST_WORKER.run(|| Ok(std::thread::current().id())).unwrap();
        assert_eq!(a, c, "a panicking job does not kill the worker");
    }

    /// The old race: a job queued while a panicking job unwound the thread was dropped with the
    /// receiver. Queue jobs from other threads right behind a panicking one; none may be lost.
    #[test]
    fn jobs_queued_behind_a_panic_are_not_lost() {
        static RACE: Worker = Worker::new("linux-worker-race");
        for _ in 0..50 {
            let followers: Vec<_> = (0..4)
                .map(|i| std::thread::spawn(move || RACE.run(move || Ok(i))))
                .collect();
            let _ = RACE.run(|| -> Result<()> { panic!("boom") });
            for (i, h) in followers.into_iter().enumerate() {
                assert_eq!(h.join().unwrap().unwrap(), i);
            }
        }
    }
}
