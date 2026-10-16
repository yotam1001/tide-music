//! Native slider input and reused-buffer rendering, without a desktop or account.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    Key, PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, Rgb8Pixel};
use std::{cell::Cell, rc::Rc};

slint::include_modules!();

const WIDTH: usize = 700;
const HEIGHT: usize = 132;
const ACCENT: Rgb8Pixel = Rgb8Pixel::new(179, 61, 96);
const RAIL: Rgb8Pixel = Rgb8Pixel::new(68, 70, 77);

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn draw(window: &MinimalSoftwareWindow, pixels: &mut [Rgb8Pixel]) {
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(pixels, WIDTH);
        }));
    }
}

fn full_reference(window: &MinimalSoftwareWindow) -> Vec<Rgb8Pixel> {
    let mut pixels = vec![Rgb8Pixel::default(); WIDTH * HEIGHT];
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
        renderer.render(&mut pixels, WIDTH);
        renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
    }));
    pixels
}

fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn key(ui: &AppWindow, key: Key) {
    let text: slint::SharedString = key.into();
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn bounds(
    pixels: &[Rgb8Pixel],
    y: usize,
    from: usize,
    to: usize,
    color: Rgb8Pixel,
) -> (usize, usize) {
    let xs: Vec<_> = (from..to)
        .filter(|&x| pixels[y * WIDTH + x] == color)
        .collect();
    (
        *xs.first().expect("slider pixels missing"),
        *xs.last().unwrap(),
    )
}

fn label_crop(pixels: &[Rgb8Pixel], y: usize) -> Vec<Rgb8Pixel> {
    (y.saturating_sub(12)..(y + 13).min(HEIGHT))
        .flat_map(|row| {
            pixels[row * WIDTH + WIDTH - 65..row * WIDTH + WIDTH - 15]
                .iter()
                .copied()
        })
        .collect()
}

fn export(name: &str, pixels: &[Rgb8Pixel]) {
    if let Some(path) = std::env::var_os("TIDE_PLAYER_INDICATORS_DIR") {
        let path = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&path).unwrap();
        let bytes = pixels
            .iter()
            .flat_map(|pixel| [pixel.r, pixel.g, pixel.b])
            .collect();
        image::RgbImage::from_raw(WIDTH as u32, HEIGHT as u32, bytes)
            .unwrap()
            .save(path.join(format!("{name}.png")))
            .unwrap();
    }
}

#[test]
fn native_sliders_keep_external_playback_updates_and_paint_volume_without_resize() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    ui.set_collapsed(true);
    ui.set_light_mode(false);
    ui.set_track_title("Synthetic indicator fixture".into());
    ui.set_duration(180.0);
    ui.set_position(30.0);
    ui.set_volume(25.0);
    let last_seek = Rc::new(Cell::new(-1.0_f32));
    let recorded = last_seek.clone();
    ui.on_seek(move |position| recorded.set(position));
    let volume_callbacks = Rc::new(Cell::new(0));
    let recorded = volume_callbacks.clone();
    let weak = ui.as_weak();
    ui.on_change_volume(move |volume| {
        recorded.set(recorded.get() + 1);
        if let Some(ui) = weak.upgrade() {
            // Same UI update as the app's volume handler; no audio is started.
            ui.set_volume(volume.clamp(0.0, 100.0));
        }
    });
    ui.show().unwrap();
    let mut pixels = vec![Rgb8Pixel::default(); WIDTH * HEIGHT];
    draw(&window, &mut pixels);
    export("initial", &pixels);

    // Locate the code-native slider paint rather than relying on font geometry.
    let y = (60..HEIGHT - 8)
        .max_by_key(|&y| {
            (50..500)
                .filter(|&x| pixels[y * WIDTH + x] == ACCENT)
                .count()
        })
        .unwrap();
    let (left, _) = bounds(&pixels, y, 50, 500, ACCENT);
    let (_, right) = bounds(&pixels, y, 50, 500, RAIL);
    assert!(right - left > 150, "could not locate progress track");
    let mut failures = Vec::new();

    // Input goes through the real std-widgets Slider, not invoke_seek(). The
    // keyboard End action also guarantees an exact maximum value after focus.
    click(&ui, (right - 2) as f32, y as f32);
    key(&ui, Key::End);
    assert!(
        (last_seek.get() - 180.0).abs() < 0.01,
        "native seek did not reach MAX"
    );
    ui.set_position(40.0);
    draw(&window, &mut pixels);
    export("progress-after-seek-partial", &pixels);
    let partial_right = bounds(&pixels, y, 50, 500, ACCENT).1;
    let full = full_reference(&window);
    export("progress-after-seek-full", &full);
    let full_right = bounds(&full, y, 50, 500, ACCENT).1;
    let expected = left as f32 + (right + 1 - left) as f32 * (40.0 / 180.0) + 6.0;
    if (partial_right as f32 - expected).abs() > 4.0 || (full_right as f32 - expected).abs() > 4.0 {
        failures.push(format!("progress binding lost after native MAX seek: root={}, expected thumb right~{expected:.1}, partial={partial_right}, full={full_right}", ui.get_position()));
    }

    // Reuse the same window and buffer when a different song starts.
    ui.set_position(0.0);
    ui.set_duration(240.0);
    draw(&window, &mut pixels);
    export("new-track-partial", &pixels);
    let reset_right = bounds(&pixels, y, 50, 500, ACCENT).1;
    if reset_right > left + 10 {
        failures.push(format!("new track did not reset progress thumb: root={}, thumb right={reset_right}, track left={left}", ui.get_position()));
    }

    // Check text repaint independently even when the progress regression fails.
    let before_volume = label_crop(&pixels, y);
    let (volume_left, _) = bounds(&pixels, y, 500, WIDTH - 65, ACCENT);
    let (_, volume_right) = bounds(&pixels, y, 500, WIDTH - 65, RAIL);
    let target_x = (volume_left - 10) as f32 + (volume_right + 1 - volume_left + 20) as f32 * 0.75;
    click(&ui, target_x, y as f32);
    assert!(
        volume_callbacks.get() > 0,
        "native volume callback did not fire"
    );
    assert!(
        (70.0..80.0).contains(&ui.get_volume()),
        "volume model did not update"
    );
    draw(&window, &mut pixels);
    export("volume-partial", &pixels);
    let partial_label = label_crop(&pixels, y);
    // First capture partial output, THEN force a full render. Neither operation
    // resizes the window or rebuilds the component/property graph.
    let full = full_reference(&window);
    export("volume-full", &full);
    let full_label = label_crop(&full, y);
    assert_ne!(
        before_volume, full_label,
        "volume text reference did not change"
    );
    if partial_label != full_label {
        failures.push(format!(
            "volume label reused-buffer paint differs from full render at model volume {}",
            ui.get_volume()
        ));
    }

    // Repeat the interaction/clock/track-handoff sequence that accumulates in
    // long listening sessions. This advances UI state, not wall-clock audio.
    for song in 0..30 {
        ui.set_track_title(format!("Synthetic handoff {song}").into());
        let duration = 180.0 + (song % 5) as f32 * 30.0;
        ui.set_duration(duration);
        ui.set_position(0.0);
        draw(&window, &mut pixels);
        click(&ui, (right - 2) as f32, y as f32);
        key(&ui, Key::End);
        for position in [0.0, 15.0, 40.0, 90.0] {
            ui.set_position(position);
            draw(&window, &mut pixels);
            let thumb = bounds(&pixels, y, 50, 500, ACCENT).1;
            let expected = left as f32 + (right + 1 - left) as f32 * position / duration + 6.0;
            assert!(
                (thumb as f32 - expected).abs() <= 4.0,
                "progress stopped following the clock at handoff {song}, position {position}/{duration}: thumb={thumb}, expected={expected}, model={}", ui.get_position()
            );
        }
        let value = [9.0, 25.0, 75.0, 100.0, 0.0][song % 5];
        ui.set_volume(value);
        draw(&window, &mut pixels);
        let partial = label_crop(&pixels, y);
        let full = full_reference(&window);
        assert_eq!(
            partial,
            label_crop(&full, y),
            "volume paint stale at handoff {song}"
        );
    }
    assert_eq!(
        window.size(),
        PhysicalSize::new(WIDTH as u32, HEIGHT as u32)
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
