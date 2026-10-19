#![expect(
    clippy::collapsible_if,
    reason = "Repository guidelines require explicit nested control flow"
)]

#[path = "../src/code_editor.rs"]
#[expect(
    dead_code,
    reason = "The renderer benchmark does not exercise application editor actions"
)]
mod code_editor;
#[path = "../src/inline_output.rs"]
#[expect(
    dead_code,
    reason = "The editor benchmark does not generate CAD results"
)]
mod inline_output;
#[path = "../src/interface.rs"]
#[expect(
    dead_code,
    reason = "The renderer benchmark does not draw application chrome"
)]
mod interface;
#[path = "../src/modal.rs"]
#[expect(
    dead_code,
    reason = "The renderer benchmark does not dispatch application modal actions"
)]
mod modal;
#[path = "../src/parameters.rs"]
#[expect(
    dead_code,
    reason = "The renderer benchmark does not edit model parameters"
)]
mod parameters;
#[path = "../src/search.rs"]
mod search;
#[path = "../src/syntax.rs"]
mod syntax;
#[path = "../src/text_buffer.rs"]
mod text_buffer;

use eframe::egui;
use std::time::Instant;
use text_buffer::Buffer;

fn main() {
    for (name, source) in [
        ("100_kib", "local width = 100 -- λ\n".repeat(4_500)),
        ("100k_lines", "local width = 100 -- λ\n".repeat(100_000)),
        ("1m_character_line", "x".repeat(1_000_000)),
    ] {
        let bytes = source.len();
        let started = Instant::now();
        let mut buffer = Buffer::from(source.as_str());
        assert!(!buffer.is_empty());
        let load_ms = started.elapsed().as_secs_f64() * 1000.0;
        let revision = buffer.revision;
        let mut edits = Vec::with_capacity(2000);
        for index in 0..2000 {
            let position = index * 997 % buffer.rope.len_chars();
            let started = Instant::now();
            assert!(buffer.replace(position..position, "x", position));
            edits.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        }
        assert_ne!(revision, buffer.revision);
        for _ in 0..1024 {
            assert!(buffer.undo(false).is_some());
        }
        for _ in 0..1024 {
            assert!(buffer.undo(true).is_some());
        }
        let context = egui::Context::default();
        let mut state = code_editor::State::default();
        state.jump_to(0..0);
        let mut frames = Vec::with_capacity(200);
        for index in 0..205 {
            let position = index * 997 % buffer.rope.len_chars();
            assert!(buffer.replace(position..position, "x", position));
            let started = Instant::now();
            context
                .run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(440.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        code_editor::show(ui, &mut buffer, &mut state);
                    },
                )
                .drop_without_applying_deltas();
            if index >= 5 {
                frames.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        edits.sort_by(f64::total_cmp);
        frames.sort_by(f64::total_cmp);
        println!(
            "{name}: {bytes} bytes; load={load_ms:.3}ms; edit p50={:.3}us p95={:.3}us max={:.3}us; visible editor frame p50={:.3}ms p95={:.3}ms max={:.3}ms",
            edits[1000], edits[1900], edits[1999], frames[100], frames[190], frames[199]
        );
    }
}
