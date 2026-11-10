use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use std::rc::Rc;

slint::include_modules!();

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn draw(window: &MinimalSoftwareWindow, pixels: &mut [Rgb8Pixel], width: usize) {
    slint::platform::update_timers_and_animations();
    window.request_redraw();
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(pixels, width);
    }));
}

#[test]
fn restore_repaints_a_cleared_reused_buffer_without_resizing() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    // Pixel assertions need settled frames, not mid-transition ones.
    ui.set_motion(false);
    window.set_size(PhysicalSize::new(780, 720));
    ui.show().unwrap();
    let mut pixels = vec![Rgb8Pixel::default(); 780 * 720];

    for light in [false, true] {
        ui.set_light_mode(light);
        // Settle property callbacks and the initial partial-rendering cache.
        for _ in 0..3 {
            draw(&window, &mut pixels, 780);
        }
        // Compare against a complete render of the same scene. Incremental
        // frames after a theme switch can differ from it by a few antialiased
        // icon pixels; restore must produce the correct image either way.
        let mut expected = vec![Rgb8Pixel::default(); 780 * 720];
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
            renderer.render(&mut expected, 780);
            renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
        }));
        pixels.copy_from_slice(&expected);
        for _ in 0..4 {
            window.set_minimized(true);
            slint::platform::update_timers_and_animations();
            // Reproduce Windows discarding the contents while buffer age still
            // reports ReusedBuffer. Do not resize or take a snapshot: either
            // would clear the renderer cache and hide the regression.
            pixels.fill(Rgb8Pixel::default());
            window.set_minimized(false);
            draw(&window, &mut pixels, 780);
            assert_eq!(window.size(), PhysicalSize::new(780, 720));
            let differing: Vec<usize> = (0..pixels.len())
                .filter(|&i| pixels[i] != expected[i])
                .collect();
            assert!(
                differing.is_empty(),
                "Restore left cleared or stale pixels: {} differ, light={light}, from ({}, {}) to ({}, {})",
                differing.len(),
                differing.first().map_or(0, |i| i % 780),
                differing.first().map_or(0, |i| i / 780),
                differing.last().map_or(0, |i| i % 780),
                differing.last().map_or(0, |i| i / 780),
            );
        }
    }
}
