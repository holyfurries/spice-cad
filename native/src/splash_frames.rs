use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureId};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const PAGES: [&[u8]; 10] = [
    include_bytes!("../assets/brand/frames/atlas-000.png"),
    include_bytes!("../assets/brand/frames/atlas-001.png"),
    include_bytes!("../assets/brand/frames/atlas-002.png"),
    include_bytes!("../assets/brand/frames/atlas-003.png"),
    include_bytes!("../assets/brand/frames/atlas-004.png"),
    include_bytes!("../assets/brand/frames/atlas-005.png"),
    include_bytes!("../assets/brand/frames/atlas-006.png"),
    include_bytes!("../assets/brand/frames/atlas-007.png"),
    include_bytes!("../assets/brand/frames/atlas-008.png"),
    include_bytes!("../assets/brand/frames/atlas-009.png"),
];

#[derive(Clone, Copy)]
struct Settings {
    fps: u32,
    frame_count: u32,
    frame_size: [u32; 2],
    grid: [u32; 2],
    background: Color32,
}

impl Settings {
    fn load() -> Result<Self, String> {
        let manifest: serde_json::Value =
            serde_json::from_slice(include_bytes!("../assets/brand/frames/manifest.json"))
                .map_err(|error| error.to_string())?;
        let number = |name: &str| -> Result<u32, String> {
            manifest[name]
                .as_u64()
                .and_then(|number| u32::try_from(number).ok())
                .ok_or_else(|| format!("Invalid splash {name}"))
        };
        let [red, green, blue, alpha]: [u8; 4] =
            serde_json::from_value(manifest["background_rgba"].clone())
                .map_err(|error| error.to_string())?;
        let settings = Self {
            fps: number("fps")?,
            frame_count: number("frame_count")?,
            frame_size: serde_json::from_value(manifest["frame_size"].clone())
                .map_err(|error| error.to_string())?,
            grid: [number("columns")?, number("rows")?],
            background: Color32::from_rgba_unmultiplied(red, green, blue, alpha),
        };
        if number("version")? != 1
            || !(1..=120).contains(&settings.fps)
            || !(1..=600).contains(&settings.frame_count)
            || settings
                .frame_size
                .iter()
                .any(|size| !(1..=1024).contains(size))
            || settings.grid.iter().any(|size| !(1..=8).contains(size))
        {
            return Err("Invalid shared splash limits".into());
        }
        let [width, height] = settings.atlas_size();
        if width > 2048
            || height > 2048
            || settings.background.a() != 255
            || settings.frame_count.div_ceil(settings.frames_per_page()) as usize != PAGES.len()
        {
            return Err("Invalid shared splash atlas dimensions".into());
        }
        let pages = manifest["pages"].as_array().ok_or("Missing splash pages")?;
        if pages.len() != PAGES.len() {
            return Err("Shared splash page count differs".into());
        }
        for (index, page) in pages.iter().enumerate() {
            if page.as_str() != Some(format!("splash/frames/atlas-{index:03}.png").as_str()) {
                return Err("Shared splash page order differs".into());
            }
        }
        Ok(settings)
    }

    fn atlas_size(self) -> [u32; 2] {
        [
            self.frame_size[0] * self.grid[0],
            self.frame_size[1] * self.grid[1],
        ]
    }

    fn frames_per_page(self) -> u32 {
        self.grid[0] * self.grid[1]
    }

    fn frame(self, elapsed: Duration) -> u32 {
        (elapsed.as_nanos().saturating_mul(u128::from(self.fps)) / 1_000_000_000)
            .min(u128::from(self.frame_count - 1)) as u32
    }

    fn uv(self, frame: u32) -> Rect {
        assert!(frame < self.frame_count);
        let cell = frame % self.frames_per_page();
        let minimum = egui::pos2(
            (cell % self.grid[0]) as f32 / self.grid[0] as f32,
            (cell / self.grid[0]) as f32 / self.grid[1] as f32,
        );
        Rect::from_min_size(
            minimum,
            egui::vec2(1.0 / self.grid[0] as f32, 1.0 / self.grid[1] as f32),
        )
    }
}

enum State {
    Waiting,
    Playing {
        started: Instant,
        page_index: u32,
        texture: TextureHandle,
    },
}

type Page = Result<(u32, ColorImage), String>;

pub struct Player {
    settings: Settings,
    receiver: mpsc::Receiver<Page>,
    state: State,
}

impl Player {
    pub fn new(context: &egui::Context) -> Result<Self, String> {
        let settings = Settings::load()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let context = context.clone();
        std::thread::Builder::new()
            .name("brand-splash".into())
            .spawn(move || decode_pages(settings, sender, context))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            settings,
            receiver,
            state: State::Waiting,
        })
    }

    pub fn background(&self) -> Color32 {
        self.settings.background
    }

    pub fn frame(
        &mut self,
        context: &egui::Context,
        now: Instant,
    ) -> Result<Option<(TextureId, Rect)>, String> {
        let frame = match &self.state {
            State::Waiting => 0,
            State::Playing { started, .. } => {
                self.settings.frame(now.saturating_duration_since(*started))
            }
        };
        let target_page = frame / self.settings.frames_per_page();
        for _ in 0..PAGES.len() {
            if let State::Playing { page_index, .. } = &self.state {
                if *page_index >= target_page {
                    break;
                }
            }
            let (index, image) = match self.receiver.try_recv() {
                Ok(page) => page?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Shared splash decoder stopped before the final page".into());
                }
            };
            match &mut self.state {
                State::Waiting => {
                    if index != 0 {
                        return Err("Shared splash did not start at frame zero".into());
                    }
                    self.state = State::Playing {
                        started: now,
                        page_index: index,
                        texture: context.load_texture(
                            "holy-furries-animation",
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    };
                }
                State::Playing {
                    page_index,
                    texture,
                    ..
                } => {
                    if index != *page_index + 1 {
                        return Err("Shared splash pages arrived out of order".into());
                    }
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *page_index = index;
                }
            }
        }
        match &self.state {
            State::Waiting => Ok(None),
            State::Playing {
                page_index,
                texture,
                started,
            } => {
                let visible = frame.min((page_index + 1) * self.settings.frames_per_page() - 1);
                if frame + 1 < self.settings.frame_count {
                    let next = Duration::from_nanos(
                        ((u64::from(frame) + 1) * 1_000_000_000)
                            .div_ceil(u64::from(self.settings.fps)),
                    );
                    context.request_repaint_after(
                        next.saturating_sub(now.saturating_duration_since(*started)),
                    );
                }
                Ok(Some((texture.id(), self.settings.uv(visible))))
            }
        }
    }
}

fn decode_pages(settings: Settings, sender: mpsc::SyncSender<Page>, context: egui::Context) {
    for (index, bytes) in PAGES.iter().enumerate() {
        let page = decode(bytes, settings.atlas_size()).map(|image| (index as u32, image));
        let failed = page.is_err();
        if sender.send(page).is_err() {
            break;
        }
        context.request_repaint();
        if failed {
            break;
        }
    }
}

fn decode(bytes: &[u8], expected: [u32; 2]) -> Result<ColorImage, String> {
    assert!(expected[0] <= 2048);
    assert!(expected[1] <= 2048);
    let size_bytes = expected[0] as usize * expected[1] as usize * 4;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: size_bytes });
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    if [reader.info().width, reader.info().height] != expected
        || reader.output_buffer_size() != Some(size_bytes)
    {
        return Err("Shared splash page has an unexpected canvas or pixel format".into());
    }
    let mut bytes = vec![0; size_bytes];
    let frame = reader
        .next_frame(&mut bytes)
        .map_err(|error| error.to_string())?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return Err("Shared splash page requires eight-bit RGBA pixels".into());
    }
    Ok(ColorImage::from_rgba_unmultiplied(
        [expected[0] as usize, expected[1] as usize],
        &bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_timing_and_page_boundaries_follow_the_shared_export() {
        let settings = Settings::load().unwrap();
        for (time_ms, frame) in [
            (0, 0),
            (16, 0),
            (17, 1),
            (133, 7),
            (134, 8),
            (1200, 72),
            (1500, 72),
            (60_000, 72),
        ] {
            assert_eq!(settings.frame(Duration::from_millis(time_ms)), frame);
            let uv = settings.uv(frame);
            assert!(Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)).contains_rect(uv));
        }
        assert_eq!(settings.uv(0), settings.uv(8));
        assert_eq!(settings.atlas_size(), [1920, 800]);
    }

    #[test]
    fn every_shared_page_decodes_and_invalid_pages_fail() {
        let settings = Settings::load().unwrap();
        for page in PAGES {
            let image = decode(page, settings.atlas_size()).unwrap();
            assert_eq!(image.size, [1920, 800]);
            assert_eq!(image.pixels[0], settings.background);
        }
        assert!(decode(PAGES[0], [1, 1]).is_err());
        assert!(decode(b"invalid", settings.atlas_size()).is_err());
    }

    #[test]
    fn decoder_stops_when_the_player_is_dismissed_with_a_full_queue() {
        let settings = Settings::load().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send(Ok((0, ColorImage::new([1, 1], vec![Color32::BLACK]))))
            .unwrap();
        let (finished, completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            decode_pages(settings, sender, egui::Context::default());
            finished.send(()).unwrap();
        });
        drop(receiver);
        completion.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn playback_reuses_one_texture_and_holds_the_final_frame_without_uploads() {
        let context = egui::Context::default();
        let settings = Settings::load().unwrap();
        let (sender, receiver) = mpsc::sync_channel(PAGES.len());
        let mut player = Player {
            settings,
            receiver,
            state: State::Waiting,
        };
        let step = |player: &mut Player, time| {
            let mut image = None;
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                image = player.frame(ui.ctx(), time).unwrap()
            });
            let uploads = output.textures_delta.set.len();
            output.drop_without_applying_deltas();
            (image, uploads)
        };
        let start = Instant::now();
        assert!(step(&mut player, start).0.is_none());
        sender
            .send(Ok((0, decode(PAGES[0], settings.atlas_size()).unwrap())))
            .unwrap();
        let first = step(&mut player, start + Duration::from_secs(1));
        let (texture, uv) = first.0.unwrap();
        assert_eq!(uv, settings.uv(0));
        assert_eq!(first.1, 1);
        let next = step(&mut player, start + Duration::from_millis(1017));
        assert_eq!(next.0, Some((texture, settings.uv(1))));
        assert_eq!(next.1, 0);
        sender
            .send(Ok((1, decode(PAGES[1], settings.atlas_size()).unwrap())))
            .unwrap();
        let next_page = step(&mut player, start + Duration::from_millis(1134));
        assert_eq!(next_page.0, Some((texture, settings.uv(8))));
        assert_eq!(next_page.1, 1);
        for (index, bytes) in PAGES.iter().enumerate().skip(2) {
            sender
                .send(Ok((
                    index as u32,
                    decode(bytes, settings.atlas_size()).unwrap(),
                )))
                .unwrap();
        }
        drop(sender);
        assert_eq!(
            step(&mut player, start + Duration::from_secs(3)).0,
            Some((texture, settings.uv(72)))
        );
        let held = step(&mut player, start + Duration::from_secs(60));
        assert_eq!(held.0, Some((texture, settings.uv(72))));
        assert_eq!(held.1, 0);
        drop(player);
        let output = context.run_ui(egui::RawInput::default(), |_| {});
        assert!(output.textures_delta.free.contains(&texture));
        output.drop_without_applying_deltas();
    }
}
