//! COM threading for the audio/media backends (skill rule 5/6): every Core Audio / WinRT call
//! runs on a dedicated MTA thread, never on the async runtime. Workers start lazily, block on
//! their queue while idle (no CPU) and live for the rest of the process.

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::platform::{PlatformError, Result};

/// A caller never waits longer than this for one job (a stuck WinRT async op must not hang a
/// command forever; the job itself keeps running on the worker).
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

type Job = Box<dyn FnOnce() + Send>;

/// Initialises COM (MTA) for the current thread; uninitialises on drop.
pub struct ComGuard;

impl ComGuard {
    pub fn init() -> Result<Self> {
        // SAFETY: called once per thread that owns the guard; paired with CoUninitialize in Drop.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            Ok(ComGuard)
        } else {
            Err(PlatformError::Os(format!(
                "CoInitializeEx failed: {}",
                hr.message()
            )))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx in `init` on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// A named MTA worker thread with a job queue.
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
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);
        let name = self.name;
        std::thread::Builder::new()
            .name(self.name.into())
            .spawn(move || {
                let com = match ComGuard::init() {
                    Ok(g) => g,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                // A panicking job must not kill the thread: jobs already queued behind it would be
                // dropped with the receiver ("call aborted"), and the thread-local state would be lost.
                // The panicking job's own caller still gets an error (its result sender is dropped).
                for job in rx {
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                        tracing::warn!(worker = name, "job panicked; worker keeps running");
                    }
                }
                drop(com);
            })
            .map_err(|e| PlatformError::Os(format!("cannot start {}: {e}", self.name)))?;
        ready_rx
            .recv()
            .map_err(|_| PlatformError::Os(format!("{} exited during start", self.name)))??;
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
                // The job is stuck inside an OS call (e.g. a hung media app). Abandon this worker
                // so later calls get a fresh thread instead of queueing behind it forever; the
                // stuck thread exits on its own once the call returns and its channel is closed.
                *self.tx.lock().unwrap_or_else(|p| p.into_inner()) = None;
                tracing::warn!(worker = self.name, "COM call timed out; respawning worker");
                Err(PlatformError::Os(format!("{} call timed out", self.name)))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(PlatformError::Os(format!("{} call aborted", self.name)))
            }
        }
    }
}

pub fn os(e: windows::core::Error) -> PlatformError {
    PlatformError::Os(e.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_WORKER: Worker = Worker::new("com-test");

    #[test]
    fn worker_runs_jobs_and_survives_errors() {
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
}
