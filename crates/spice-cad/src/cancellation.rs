use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Default)]
pub struct Generation(Arc<AtomicU64>);

impl Generation {
    pub fn advance(&self) {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .expect("Preview generation exhausted");
    }

    pub fn token(&self) -> Token {
        Token {
            generation: self.0.load(Ordering::Acquire),
            current: Arc::clone(&self.0),
            #[cfg(test)]
            checkpoints: None,
        }
    }
}

#[derive(Clone)]
pub struct Token {
    generation: u64,
    current: Arc<AtomicU64>,
    #[cfg(test)]
    checkpoints: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl Token {
    pub fn cancelled(&self) -> bool {
        self.current.load(Ordering::Acquire) != self.generation
    }

    pub fn checkpoint(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(checkpoints) = &self.checkpoints {
            if checkpoints
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    count.checked_sub(1)
                })
                .is_err()
            {
                self.current.store(self.generation + 1, Ordering::Release);
            }
        }
        if self.cancelled() {
            return Err("Preview superseded".into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn after_checkpoints(mut self, count: usize) -> Self {
        self.checkpoints = Some(Arc::new(std::sync::atomic::AtomicUsize::new(count)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_generation_cancels_only_older_tickets() {
        let generation = Generation::default();
        let first = generation.token();
        assert!(!first.cancelled());
        generation.advance();
        assert!(first.checkpoint().is_err());
        assert!(generation.token().checkpoint().is_ok());
    }
}
