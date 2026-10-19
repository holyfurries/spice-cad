use std::time::Instant;

#[derive(Clone, Copy)]
pub struct Input {
    pub revision: u64,
    pub at: Instant,
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
    latency: Samples,
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
        if let Some((revision, latency)) = metrics.input_submission {
            if self.last_input != Some(revision) {
                self.last_input = Some(revision);
                self.latency.record(latency);
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
        if let Some([p50, p95, p99, maximum]) = self.latency.summary() {
            eprintln!(
                "Profile: slider_latency accepted_samples={} retained_samples={} p50_ms={p50:.3} p95_ms={p95:.3} p99_ms={p99:.3} max_ms={maximum:.3} boundary=control_change_to_gpu_prepare",
                self.latency.count,
                self.latency.count.min(SAMPLE_COUNT_MAX as u64)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            input_submission: Some((1, 2.0)),
            ..Default::default()
        };
        profile.frame([1400, 900], 0.0, metrics);
        profile.frame([1400, 900], 0.0, metrics);
        assert_eq!(profile.latency.count, 1);
    }
}
