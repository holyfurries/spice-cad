use crate::{
    interface,
    print_preparation::{Finding, Report, Severity},
};
use eframe::egui;

#[derive(Default)]
pub enum State {
    #[default]
    Idle,
    Running {
        revision: u64,
    },
    Ready {
        revision: u64,
        report: Report,
        findings: Vec<Finding>,
        selected: Option<usize>,
    },
    Failed {
        revision: u64,
        message: String,
    },
}

pub enum Action {
    Run,
    Focus(usize),
}

impl State {
    pub fn start(&mut self, revision: u64) {
        assert!(revision > 0);
        *self = Self::Running { revision };
    }

    pub fn finish(&mut self, revision: u64, result: Result<Report, String>) {
        if !matches!(self, Self::Running { revision: active } if *active == revision) {
            return;
        }
        *self = match result {
            Ok(report) => {
                let findings = report.findings();
                Self::Ready {
                    revision,
                    report,
                    findings,
                    selected: None,
                }
            }
            Err(message) => Self::Failed { revision, message },
        };
    }

    pub fn fail(&mut self, message: String) {
        if let Self::Running { revision } = self {
            let revision = *revision;
            self.finish(revision, Err(message));
        }
    }

    pub fn finding(&self, revision: Option<u64>, index: usize) -> Option<&Finding> {
        match self {
            Self::Ready {
                revision: checked,
                findings,
                ..
            } if revision == Some(*checked) => findings.get(index),
            _ => None,
        }
    }

    pub fn selection(&self, revision: Option<u64>) -> Option<&Finding> {
        match self {
            Self::Ready {
                selected: Some(index),
                ..
            } => self.finding(revision, *index),
            _ => None,
        }
    }

    pub fn clear_selection(&mut self) {
        if let Self::Ready { selected, .. } = self {
            *selected = None;
        }
    }

    pub fn select(&mut self, revision: Option<u64>, index: usize) -> bool {
        if self.finding(revision, index).is_none() {
            return false;
        }
        if let Self::Ready { selected, .. } = self {
            *selected = Some(index);
        }
        true
    }

    pub fn show(
        &mut self,
        context: &egui::Context,
        open: &mut bool,
        revision: Option<u64>,
        can_run: bool,
    ) -> Option<Action> {
        let mut action = None;
        egui::Window::new("Print checks").open(open).default_pos(egui::pos2(24.0, 105.0)).default_width(480.0).default_height(600.0).show(context, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Bambu Lab P1S").strong());
                ui.label(egui::RichText::new("256 × 256 × 256 mm").color(interface::MUTED));
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(can_run, egui::Button::new(if matches!(self, Self::Running { .. }) { "Checking…" } else { "Run print checks" })).on_disabled_hover_text("Wait for the current model to finish building before checking.").clicked() { action = Some(Action::Run); }
                if ui.add_enabled(matches!(self, Self::Ready { selected: Some(_), .. }), egui::Button::new("Clear highlight")).clicked() { self.clear_selection(); }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("print_check_results").max_height((ui.available_height() - 76.0).max(80.0)).auto_shrink([false; 2]).show(ui, |ui| match self {
                Self::Idle => { ui.label("Run checks to inspect bed placement, part size, sampled walls, assembly gaps, and exact solid overlaps."); },
                Self::Running { revision: checked } => {
                    ui.label("Checking in the background… You can continue editing and moving the camera.");
                    if revision != Some(*checked) { ui.colored_label(interface::WARNING, "The model changed. Run again when this check finishes."); }
                }
                Self::Failed { revision: checked, message } => {
                    ui.colored_label(interface::WARNING, "Print checks could not finish");
                    if revision != Some(*checked) { ui.small("This error belongs to an older model."); }
                    ui.label(message.as_str());
                }
                Self::Ready { revision: checked, report, findings, selected } => {
                    let current = revision == Some(*checked);
                    if !current { ui.colored_label(interface::WARNING, "Out of date · run checks again for the current model."); }
                    let blockers = findings.iter().filter(|finding| finding.severity == Severity::Blocked).count();
                    let warnings = findings.iter().filter(|finding| finding.severity == Severity::Warning).count();
                    ui.label(format!("{} parts · {blockers} blockers · {warnings} warnings", report.part_count()));
                    if blockers == 0 { ui.small("Part envelopes and bed contact passed. Sampling does not certify printability."); }
                    let options = report.options();
                    ui.small(format!("Wall warning < {:.2} mm · gap warning < {:.2} mm", options.minimum_wall_mm, options.minimum_clearance_mm));
                    ui.small("Thresholds come from cad.print_options in your source.");
                    let assembly = report.assembly();
                    ui.small(format!("Assembly: {} pairs · {} overlaps · {} incomplete", assembly.pairs, assembly.overlaps.len(), assembly.failures.len()));
                    ui.small(format!("{} B-rep intersections · {} cached pairs", assembly.boolean_tests, assembly.cache_hits));
                    ui.small("Overlap warning > 0.000001 mm³; zero-volume contact is allowed.");
                    if options.minimum_clearance_mm == 0.0 { ui.small("Assembly gap checks are disabled in the source."); }
                    if report.skipped_pairs() > 0 { ui.colored_label(interface::WARNING, format!("{} assembly pairs skipped: sampling budget exhausted.", report.skipped_pairs())); }
                    ui.add_space(8.0);
                    for (index, finding) in findings.iter().enumerate() {
                        let (prefix, color) = match finding.severity { Severity::Blocked => ("BLOCKED", interface::WARNING), Severity::Warning => ("WARNING", interface::WARNING), Severity::Info => ("SAMPLE", interface::MUTED) };
                        ui.small(egui::RichText::new(prefix).color(color));
                        if ui.add_enabled(current, egui::Button::new(&finding.title).selected(*selected == Some(index)).wrap().min_size(egui::vec2(ui.available_width(), 28.0))).clicked() { action = Some(Action::Focus(index)); }
                        ui.small(&finding.detail);
                        ui.add_space(8.0);
                    }
                }
            });
            ui.separator();
            ui.small("Collisions use exact B-rep intersections with numerical kernel tolerances; contact without volume is allowed. Walls and gaps use bounded mesh samples. Missing warnings do not prove printability or validate a combined plate arrangement.");
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Result<Report, String> {
        crate::print_preparation::model_report(&crate::model::compile(
            "cad.show('thin',cad.box({width=10,depth=10,height=0.5}))",
        )?)
    }

    #[test]
    fn old_results_cannot_select_findings_or_replace_a_new_request() -> Result<(), String> {
        let mut state = State::default();
        state.start(1);
        state.start(2);
        state.finish(1, Ok(report()?));
        assert!(matches!(state, State::Running { revision: 2 }));
        state.finish(2, Ok(report()?));
        assert!(!state.select(Some(1), 0));
        assert!(state.select(Some(2), 0));
        assert!(state.selection(Some(2)).is_some());
        assert!(state.selection(None).is_none());
        assert!(state.selection(Some(3)).is_none());
        state.clear_selection();
        assert!(state.selection(Some(2)).is_none());
        state.start(3);
        state.fail("worker unavailable".into());
        assert!(matches!(state, State::Failed { revision: 3, .. }));
        state.start(4);
        state.finish(4, Ok(report()?));
        assert!(state.select(Some(4), 0));
        Ok(())
    }
}
