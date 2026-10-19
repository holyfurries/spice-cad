use std::time::{Duration, Instant};

pub const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq)]
enum Trigger {
    Automatic,
    Manual,
}

struct Request {
    revision: u64,
    ready_at: Instant,
    trigger: Trigger,
}

pub struct Schedule {
    automatic: bool,
    observed: Option<u64>,
    pending: Option<Request>,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            automatic: true,
            observed: None,
            pending: None,
        }
    }
}

impl Schedule {
    pub fn parameter(&mut self, revision: u64, now: Instant) {
        self.observe(revision, now);
        if self.automatic {
            self.pending = Some(Request {
                revision,
                ready_at: now,
                trigger: Trigger::Automatic,
            });
        }
    }

    pub fn automatic(&self) -> bool {
        self.automatic
    }

    pub fn observe(&mut self, revision: u64, now: Instant) -> bool {
        if self.observed == Some(revision) {
            return false;
        }
        self.observed = Some(revision);
        self.pending = self.automatic.then_some(Request {
            revision,
            ready_at: now + DEBOUNCE,
            trigger: Trigger::Automatic,
        });
        true
    }

    pub fn run(&mut self, revision: u64, now: Instant) {
        self.observed = Some(revision);
        self.pending = Some(Request {
            revision,
            ready_at: now,
            trigger: Trigger::Manual,
        });
    }

    pub fn set_automatic(&mut self, automatic: bool, revision: u64, now: Instant) {
        if automatic == self.automatic {
            return;
        }
        self.automatic = automatic;
        if automatic {
            self.observed = None;
            self.observe(revision, now);
        } else if self
            .pending
            .as_ref()
            .is_some_and(|request| request.trigger == Trigger::Automatic)
        {
            self.pending = None;
        }
    }

    pub fn reuse(&mut self, revision: u64) -> bool {
        if self.pending.as_ref().is_some_and(|request| {
            request.revision == revision && request.trigger == Trigger::Automatic
        }) {
            self.pending = None;
            return true;
        }
        false
    }

    pub fn complete(&mut self, revision: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|request| request.revision == revision)
        {
            self.pending = None;
        }
    }

    pub fn ready(&self, now: Instant) -> Option<u64> {
        self.pending
            .as_ref()
            .filter(|request| now >= request.ready_at)
            .map(|request| request.revision)
    }

    pub fn delay(&self, now: Instant) -> Option<Duration> {
        self.pending
            .as_ref()
            .map(|request| request.ready_at.saturating_duration_since(now))
    }

    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn observed(&self) -> Option<u64> {
        self.observed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_changes_run_immediately_and_respect_manual_mode() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.parameter(1, now);
        assert_eq!(schedule.ready(now), Some(1));
        schedule.parameter(2, now);
        schedule.complete(1);
        assert_eq!(schedule.ready(now), Some(2));
        assert!(!schedule.observe(2, now));
        assert_eq!(schedule.ready(now), Some(2));
        schedule.set_automatic(false, 2, now);
        schedule.parameter(3, now);
        assert!(!schedule.pending());
        schedule.run(3, now);
        assert_eq!(schedule.ready(now), Some(3));
    }

    #[test]
    fn typing_coalesces_to_one_latest_revision_after_a_pause() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.observe(1, now);
        schedule.observe(2, now + DEBOUNCE / 2);
        assert_eq!(schedule.ready(now + DEBOUNCE), None);
        assert_eq!(schedule.ready(now + DEBOUNCE * 2), Some(2));
        schedule.complete(1);
        assert_eq!(schedule.ready(now + DEBOUNCE * 2), Some(2));
        schedule.complete(2);
        schedule.observe(2, now + DEBOUNCE * 3);
        assert!(!schedule.pending());
    }

    #[test]
    fn manual_mode_runs_explicitly_and_retries_without_automatic_error_loops() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.observe(1, now);
        schedule.set_automatic(false, 1, now);
        schedule.observe(2, now);
        assert!(!schedule.pending());
        schedule.run(2, now);
        assert_eq!(schedule.ready(now), Some(2));
        schedule.complete(2);
        schedule.observe(2, now);
        assert!(!schedule.pending());
        schedule.run(2, now);
        assert_eq!(schedule.ready(now), Some(2));
        schedule.set_automatic(true, 2, now);
        assert!(schedule.reuse(2));
        assert!(!schedule.reuse(2));
        assert!(!schedule.pending());
        schedule.observe(3, now);
        assert_eq!(schedule.ready(now + DEBOUNCE), Some(3));
    }
}
