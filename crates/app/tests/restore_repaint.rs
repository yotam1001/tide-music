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
    window.set_size(PhysicalSize::new(780, 720));
    ui.show().unwrap();
    let mut pixels = vec![Rgb8Pixel::default(); 780 * 720];

    for light in [false, true] {
        ui.set_light_mode(light);
        // Settle property callbacks and the initial partial-rendering cache.
        for _ in 0..3 {
            draw(&window, &mut pixels, 780);
        }
        let expected = pixels.clone();
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
            assert!(pixels == expected, "Restore left cleared or stale pixels");
        }
    }
}
