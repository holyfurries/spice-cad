use std::time::Instant;

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
}

impl Profile {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            size: [0; 2],
            frame_count: 0,
            model_uploaded: false,
        }
    }

    pub fn frame(&mut self, size: [u32; 2], cpu_ms: f64, metrics: crate::viewport::Metrics) {
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
                "Profile: {event} elapsed_ms={:.3} ui_cpu_ms={cpu_ms:.3} frames={} width={} height={} uploads={} geometry_allocations={} geometry_bytes={} uniform_writes={}",
                self.started.elapsed().as_secs_f64() * 1000.0,
                self.frame_count,
                size[0],
                size[1],
                metrics.uploads,
                metrics.buffer_allocations,
                metrics.geometry_bytes,
                metrics.uniform_writes,
            );
        }
        self.model_uploaded = metrics.uploads > 0;
        self.size = size;
    }
}
