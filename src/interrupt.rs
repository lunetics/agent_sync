//! The `INT`, `TERM`, and `HUP` traps a transaction arms in `lib/sync.sh` and
//! `rollback`: a signal is recorded instead of killing the process, the run
//! stops at its next step and restores, and then dies of the same signal.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use signal_hook::SigId;
use signal_hook::consts::signal;

#[cfg(unix)]
const SIGNALS: [i32; 3] = [signal::SIGINT, signal::SIGTERM, signal::SIGHUP];
#[cfg(windows)]
const SIGNALS: [i32; 2] = [signal::SIGINT, signal::SIGTERM];

/// Trapped signals until dropped.
pub struct Interrupt {
    received: Arc<AtomicUsize>,
    ids: Vec<SigId>,
}

impl Interrupt {
    /// Records `INT`, `TERM`, and `HUP` from here on. A signal that cannot be
    /// trapped keeps its default action.
    pub fn arm() -> Self {
        let received = Arc::new(AtomicUsize::new(0));
        let ids = SIGNALS
            .iter()
            .filter_map(|&sig| {
                signal_hook::flag::register_usize(sig, Arc::clone(&received), sig as usize).ok()
            })
            .collect();
        Self { received, ids }
    }

    /// The signal received since `arm`, if any.
    pub fn received(&self) -> Option<i32> {
        match self.received.load(Ordering::SeqCst) {
            0 => None,
            sig => i32::try_from(sig).ok(),
        }
    }

    /// `kill -$sig $$` after the trap: the default action runs, so the parent
    /// sees the signal, not an exit status. Returns only if it did not.
    pub fn resend(&mut self, sig: i32) {
        self.disarm();
        let _ = signal_hook::low_level::emulate_default_handler(sig);
    }

    fn disarm(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// `128 + n`, the status Bash reports for a death by signal `n`.
pub fn status(sig: i32) -> u8 {
    u8::try_from(128 + sig).unwrap_or(u8::MAX)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn a_trapped_signal_is_recorded_instead_of_ending_the_process() {
        let interrupt = Interrupt::arm();
        assert_eq!(interrupt.received(), None);
        signal_hook::low_level::raise(signal::SIGHUP).unwrap();
        assert_eq!(interrupt.received(), Some(signal::SIGHUP));
        assert_eq!(status(signal::SIGHUP), 129);
        assert_eq!(status(signal::SIGINT), 130);
    }
}
