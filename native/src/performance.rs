use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const SWEEP_CHANGE_COUNT: u32 = 300;
const SWEEP_INTERVAL: Duration = Duration::from_millis(20);
const SWEEP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Sweep {
    deadline: Instant,
    started: Option<Instant>,
    last: Option<(u32, u64)>,
    changes: u32,
    completed: Arc<AtomicBool>,
}

impl Sweep {
    pub fn new(now: Instant, completed: Arc<AtomicBool>) -> Self {
        Self {
            deadline: now + SWEEP_TIMEOUT,
            started: None,
            last: None,
            changes: 0,
            completed,
        }
    }

    pub fn next(&mut self, now: Instant) -> Option<(u32, f64)> {
        let started = *self.started.get_or_insert(now);
        let index = (now.duration_since(started).as_millis() / SWEEP_INTERVAL.as_millis())
            .min(u128::from(SWEEP_CHANGE_COUNT - 1)) as u32;
        if self.last.is_some_and(|(last, _)| index <= last) {
            return None;
        }
        let phase = index % 100;
        let value = if phase <= 50 {
            phase * 2
        } else {
            (100 - phase) * 2
        };
        Some((index, f64::from(value)))
    }

    pub fn applied(&mut self, index: u32, revision: u64) {
        assert!(index < SWEEP_CHANGE_COUNT);
        assert!(self.last.is_none_or(|(previous, _)| index > previous));
        self.last = Some((index, revision));
        self.changes += 1;
    }

    pub fn finish(&self, prepared_revision: Option<u64>, now: Instant) -> bool {
        let complete = self.last.is_some_and(|(index, revision)| {
            index == SWEEP_CHANGE_COUNT - 1 && Some(revision) == prepared_revision
        });
        if complete || now >= self.deadline {
            self.completed.store(complete, Ordering::Release);
            eprintln!(
                "Profile: preview_sweep status={} scheduled_changes={} applied_changes={} elapsed_ms={:.3}",
                if complete { "complete" } else { "timeout" },
                SWEEP_CHANGE_COUNT,
                self.changes,
                self.started
                    .map_or(0.0, |started| now.duration_since(started).as_secs_f64()
                        * 1000.0)
            );
            return true;
        }
        false
    }

    pub fn delay(&self, now: Instant) -> Duration {
        self.deadline
            .saturating_duration_since(now)
            .min(SWEEP_INTERVAL)
    }

    pub fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
}

#[derive(Clone, Copy)]
pub struct Input {
    pub revision: u64,
    pub at: Instant,
    pub dispatched_ms: f64,
    pub worker_started_ms: f64,
    pub worker_finished_ms: f64,
    pub received_ms: f64,
}

impl Input {
    pub fn new(revision: u64, at: Instant) -> Self {
        Self {
            revision,
            at,
            dispatched_ms: 0.0,
            worker_started_ms: 0.0,
            worker_finished_ms: 0.0,
            received_ms: 0.0,
        }
    }

    pub fn elapsed_ms(self, at: Instant) -> f64 {
        at.duration_since(self.at).as_secs_f64() * 1000.0
    }

    pub fn prepared(self, at: Instant) -> Latency {
        let times = [
            0.0,
            self.dispatched_ms,
            self.worker_started_ms,
            self.worker_finished_ms,
            self.received_ms,
            self.elapsed_ms(at),
        ];
        assert!(times.iter().all(|value| value.is_finite()));
        assert!(times.windows(2).all(|pair| pair[0] <= pair[1]));
        let mut milliseconds = [0.0; 6];
        milliseconds[0] = times[5];
        for (index, pair) in times.windows(2).enumerate() {
            milliseconds[index + 1] = pair[1] - pair[0];
        }
        Latency {
            revision: self.revision,
            milliseconds,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Latency {
    pub revision: u64,
    pub milliseconds: [f64; 6],
}

const SAMPLE_COUNT_MAX: usize = 512;

struct Samples {
    values: [f64; SAMPLE_COUNT_MAX],
    count: u64,
}

impl Default for Samples {
    fn default() -> Self {
        Self {
            values: [0.0; SAMPLE_COUNT_MAX],
            count: 0,
        }
    }
}

impl Samples {
    fn record(&mut self, value: f64) {
        assert!(value.is_finite());
        assert!(value >= 0.0);
        self.values[(self.count % SAMPLE_COUNT_MAX as u64) as usize] = value;
        self.count += 1;
    }

    fn summary(&self) -> Option<[f64; 4]> {
        let count = self.count.min(SAMPLE_COUNT_MAX as u64) as usize;
        if count == 0 {
            return None;
        }
        let mut sorted = self.values;
        sorted[..count].sort_unstable_by(f64::total_cmp);
        Some([
            sorted[count.div_ceil(2) - 1],
            sorted[(count * 95).div_ceil(100) - 1],
            sorted[(count * 99).div_ceil(100) - 1],
            sorted[count - 1],
        ])
    }
}

pub fn stage(started: Option<Instant>, name: &str) {
    if let Some(started) = started {
        eprintln!(
            "Profile: {name} elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
}

pub struct Profile {
    started: Instant,
    size: [u32; 2],
    frame_count: u64,
    model_uploaded: bool,
    last_input: Option<u64>,
    latency: [Samples; 6],
}

impl Profile {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            size: [0; 2],
            frame_count: 0,
            model_uploaded: false,
            last_input: None,
            latency: Default::default(),
        }
    }

    pub fn frame(&mut self, size: [u32; 2], cpu_ms: f64, metrics: crate::viewport::Metrics) {
        if let Some(latency) = metrics.input_latency {
            if self.last_input != Some(latency.revision) {
                self.last_input = Some(latency.revision);
                for (samples, value) in self.latency.iter_mut().zip(latency.milliseconds) {
                    samples.record(value);
                }
            }
        }
        self.frame_count += 1;
        let event = if self.frame_count == 1 {
            Some("first_ui")
        } else if metrics.uploads > 0 && !self.model_uploaded {
            Some("first_model_upload")
        } else if size != self.size {
            Some("resize")
        } else {
            None
        };
        if let Some(event) = event {
            eprintln!(
                "Profile: {event} elapsed_ms={:.3} ui_cpu_ms={cpu_ms:.3} frames={} width={} height={} uploads={} geometry_allocations={} geometry_bytes={} uniform_writes={} placement_uploads={} placement_bytes={}",
                self.started.elapsed().as_secs_f64() * 1000.0,
                self.frame_count,
                size[0],
                size[1],
                metrics.uploads,
                metrics.buffer_allocations,
                metrics.geometry_bytes,
                metrics.uniform_writes,
                metrics.placement_writes,
                metrics.placement_bytes,
            );
        }
        self.model_uploaded = metrics.uploads > 0;
        self.size = size;
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        for (boundary, samples) in [
            "control_change_to_gpu_prepare",
            "control_change_to_dispatch",
            "worker_queue",
            "worker_build",
            "result_to_ui",
            "ui_to_gpu_prepare",
        ]
        .into_iter()
        .zip(&self.latency)
        {
            let Some([p50, p95, p99, maximum]) = samples.summary() else {
                continue;
            };
            eprintln!(
                "Profile: slider_latency accepted_samples={} retained_samples={} p50_ms={p50:.3} p95_ms={p95:.3} p99_ms={p99:.3} max_ms={maximum:.3} boundary={boundary}",
                samples.count,
                samples.count.min(SAMPLE_COUNT_MAX as u64)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sweep_coalesces_ticks_and_waits_for_the_final_renderer_revision() {
        let at = Instant::now();
        let completed = Arc::new(AtomicBool::new(false));
        let mut sweep = Sweep::new(at, Arc::clone(&completed));
        assert_eq!(sweep.next(at), Some((0, 0.0)));
        sweep.applied(0, 10);
        assert_eq!(sweep.next(at), None);
        assert_eq!(sweep.next(at + Duration::from_millis(60)), Some((3, 6.0)));
        sweep.applied(3, 11);
        assert_eq!(sweep.next(at + Duration::from_secs(8)), Some((299, 2.0)));
        sweep.applied(299, 12);
        assert!(!sweep.finish(Some(11), at + Duration::from_secs(8)));
        assert!(!completed.load(Ordering::Acquire));
        assert!(sweep.finish(Some(12), at + Duration::from_secs(8)));
        assert!(completed.load(Ordering::Acquire));
        let stalled = Sweep::new(at, Arc::new(AtomicBool::new(false)));
        assert!(stalled.finish(None, at + SWEEP_TIMEOUT));
        assert!(!stalled.completed.load(Ordering::Acquire));
    }

    #[test]
    fn percentiles_are_bounded_and_duplicate_frames_do_not_add_samples() {
        let mut samples = Samples::default();
        assert!(samples.summary().is_none());
        for value in 1..=100 {
            samples.record(f64::from(value));
        }
        assert_eq!(samples.summary(), Some([50.0, 95.0, 99.0, 100.0]));
        for value in 101..=600 {
            samples.record(f64::from(value));
        }
        assert_eq!(samples.summary(), Some([344.0, 575.0, 595.0, 600.0]));
        let mut profile = Profile::new(Instant::now());
        let metrics = crate::viewport::Metrics {
            input_latency: Some(Latency {
                revision: 1,
                milliseconds: [2.0; 6],
            }),
            ..Default::default()
        };
        profile.frame([1400, 900], 0.0, metrics);
        profile.frame([1400, 900], 0.0, metrics);
        assert!(profile.latency.iter().all(|samples| samples.count == 1));
    }

    #[test]
    fn recording_and_summarizing_stage_rings_allocate_no_heap_memory() {
        let mut profile = Profile::new(Instant::now());
        profile.frame_count = 1;
        profile.model_uploaded = true;
        profile.size = [1400, 900];
        let (_, counts) = crate::allocations::measure(|| {
            for revision in 1..=600 {
                profile.frame(
                    [1400, 900],
                    0.0,
                    crate::viewport::Metrics {
                        uploads: 1,
                        input_latency: Some(
                            Input::new(revision, profile.started).prepared(Instant::now()),
                        ),
                        ..Default::default()
                    },
                );
            }
            for samples in &profile.latency {
                std::hint::black_box(samples.summary());
            }
        });
        assert_eq!(counts, crate::allocations::Counts::default());
        assert!(profile.latency.iter().all(|samples| samples.count == 600));
    }

    #[test]
    fn stage_intervals_partition_control_to_prepare_latency() {
        let at = Instant::now();
        let mut input = Input::new(7, at);
        input.dispatched_ms = 1.0;
        input.worker_started_ms = 3.0;
        input.worker_finished_ms = 6.0;
        input.received_ms = 10.0;
        let latency = input.prepared(at + std::time::Duration::from_millis(15));
        assert_eq!(latency.revision, 7);
        assert_eq!(latency.milliseconds, [15.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(
            latency.milliseconds[1..].iter().sum::<f64>(),
            latency.milliseconds[0]
        );
    }
}
