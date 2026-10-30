//! Real native-renderer lifecycle probe, with synthetic artwork and no account.
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{
    ComponentHandle, Model, ModelRc, PhysicalSize, Rgb8Pixel, Rgba8Pixel, SharedPixelBuffer,
    VecModel,
};
use std::{cell::RefCell, rc::Rc};
use tide_music::{
    lyrics_effects::{Backdrop, BACKDROP_BYTES},
    AppWindow, LyricRow,
};

struct Platform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for Platform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn render(window: &MinimalSoftwareWindow, pixels: &mut [Rgb8Pixel]) {
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(pixels, 1140);
        });
    }
}

fn save_frame(pixels: &[Rgb8Pixel], name: &str) {
    if let Some(dir) = std::env::var_os("TIDE_UI_REVIEW_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_raw(
            1140,
            800,
            pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect(),
        )
        .unwrap()
        .save(dir.join(name))
        .unwrap();
    }
}

#[cfg(windows)]
fn private_bytes() -> usize {
    #[repr(C)]
    struct Counters {
        cb: u32,
        faults: u32,
        sizes: [usize; 9],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        faults: 0,
        sizes: [0; 9],
    };
    let size = counters.cb;
    assert_ne!(
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) },
        0
    );
    counters.sizes[8]
}
#[cfg(not(windows))]
fn private_bytes() -> usize {
    0
}

#[test]
fn repeated_open_mini_minimize_and_close_release_view_resources() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Platform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.show().unwrap();
    window.set_size(PhysicalSize::new(1140, 800));
    let backdrop = Rc::new(RefCell::new(Backdrop::default()));
    let weak = ui.as_weak();
    let released = backdrop.clone();
    ui.on_lyrics_visibility_changed(move |visible| {
        if !visible {
            released.borrow_mut().clear();
            if let Some(ui) = weak.upgrade() {
                tide_music::lyrics_effects::release_ui(&ui);
            }
        }
    });
    let mut cover = SharedPixelBuffer::<Rgba8Pixel>::new(96, 96);
    for (i, pixel) in cover.make_mut_slice().iter_mut().enumerate() {
        *pixel = Rgba8Pixel::new((i % 255) as u8, 50, (i / 96) as u8, 255);
    }
    ui.set_track_artwork(slint::Image::from_rgba8(cover));
    ui.set_track_title("Native lyrics lifecycle fixture".into());
    ui.set_track_artist("Synthetic artwork and text — no account or network".into());
    ui.set_lyrics_status("Synced lyrics".into());
    let mut pixels = vec![Rgb8Pixel::default(); 1140 * 800];
    let mut baseline = 0;
    let mut peak = 0;
    for cycle in 0..40 {
        ui.set_collapsed(false);
        ui.window().set_minimized(false);
        ui.set_settings_open(false);
        ui.set_lyrics_open(true);
        render(&window, &mut pixels);
        assert!(ui.get_lyrics_presented());
        let image = backdrop.borrow_mut().update(ui.get_track_artwork());
        assert_eq!(backdrop.borrow().allocated_bytes(), BACKDROP_BYTES);
        assert_eq!(backdrop.borrow_mut().update(ui.get_track_artwork()), image);
        ui.set_lyrics_backdrop(image);
        ui.set_lyrics_effects_active(true);
        ui.set_lyrics_lines(ModelRc::new(VecModel::from(
            (0..1000)
                .map(|i| LyricRow {
                    text: format!("Synthetic line {i} for lifecycle testing").into(),
                })
                .collect::<Vec<_>>(),
        )));
        ui.set_lyrics_synced(true);
        ui.set_lyrics_active(12);
        render(&window, &mut pixels);
        if cycle == 0 {
            ui.set_lyrics_motion_enabled(false);
            ui.set_lyrics_active(25);
            std::thread::sleep(std::time::Duration::from_millis(450));
            render(&window, &mut pixels);
            let stopped = pixels.clone();
            std::thread::sleep(std::time::Duration::from_millis(120));
            render(&window, &mut pixels);
            assert!(pixels == stopped, "Motion off must stop backdrop movement");
            save_frame(&pixels, "lyrics-effects-open.png");
            ui.set_lyrics_motion_enabled(true);
            ui.set_paused(true);
            std::thread::sleep(std::time::Duration::from_millis(450));
            render(&window, &mut pixels);
            let stopped = pixels.clone();
            std::thread::sleep(std::time::Duration::from_millis(120));
            render(&window, &mut pixels);
            assert!(
                pixels == stopped,
                "Paused playback must stop ambient movement"
            );
            ui.set_paused(false);
        }
        match cycle % 4 {
            0 => ui.set_lyrics_open(false),
            1 => ui.set_collapsed(true),
            2 => ui.window().set_minimized(true),
            _ => ui.set_settings_open(true),
        }
        render(&window, &mut pixels);
        assert!(!ui.get_lyrics_presented());
        assert!(!ui.get_lyrics_effects_active());
        assert_eq!(ui.get_lyrics_backdrop().size().width, 0);
        assert_eq!(ui.get_lyrics_lines().row_count(), 0);
        assert_eq!(backdrop.borrow().allocated_bytes(), 0);
        // Allow bounded UI property animations to settle before checking that
        // the hidden component contributes no continuously active animation.
        std::thread::sleep(std::time::Duration::from_millis(450));
        render(&window, &mut pixels);
        assert!(
            !ui.window().has_active_animations(),
            "Hidden lyrics kept an animation alive"
        );
        if cycle == 0 {
            save_frame(&pixels, "lyrics-effects-closed.png");
        }
        if cycle == 4 {
            baseline = private_bytes();
        }
        if cycle > 4 {
            peak = peak.max(private_bytes());
        }
    }
    assert!(
        peak.saturating_sub(baseline) < 20 * 1024 * 1024,
        "Closed-view private-memory growth: baseline={baseline}, peak={peak}"
    );
    println!("40 real renderer cycles; closed baseline private bytes={baseline}, peak={peak}");
}
