//! A song row's action menu must survive the cover arriving and only close when
//! the row is recycled for a different song. Native, no desktop or account.
use slint::platform::{
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    PointerEventButton, WindowEvent,
};
use slint::{ComponentHandle, LogicalPosition, Model, ModelRc, PhysicalSize, Rgb8Pixel, VecModel};
use std::rc::Rc;
use tide_music::*;

struct TestPlatform(Rc<MinimalSoftwareWindow>);
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(
        &self,
    ) -> Result<Rc<dyn slint::platform::WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn draw(window: &MinimalSoftwareWindow, width: u32, height: u32) -> Vec<Rgb8Pixel> {
    window.set_size(PhysicalSize::new(width, height));
    let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
        window.request_redraw();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
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

/// Bounding box of the pixels that differ between two frames, as
/// `(changed, left, top, right, bottom)`.
fn changed_area(
    a: &[Rgb8Pixel],
    b: &[Rgb8Pixel],
    width: usize,
) -> (usize, usize, usize, usize, usize) {
    let (mut count, mut left, mut top) = (0usize, usize::MAX, usize::MAX);
    let (mut right, mut bottom) = (0usize, 0usize);
    for (index, (a, b)) in a.iter().zip(b.iter()).enumerate() {
        if a != b {
            count += 1;
            let (x, y) = (index % width, index / width);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    (count, left, top, right, bottom)
}

/// Whether two frames match inside the given rectangle.
fn same_area(
    a: &[Rgb8Pixel],
    b: &[Rgb8Pixel],
    width: usize,
    (left, top, right, bottom): (usize, usize, usize, usize),
) -> bool {
    (top..=bottom).all(|y| {
        let row = y * width;
        (left..=right).all(|x| a[row + x] == b[row + x])
    })
}

fn row(key: &str, title: &str, artwork: slint::Image) -> TrackRow {
    TrackRow {
        key: key.into(),
        title: title.into(),
        artist: "True North".into(),
        artist_enabled: true,
        album: "Harbour".into(),
        duration: "3:36".into(),
        artwork,
        ..Default::default()
    }
}

fn cover() -> slint::Image {
    let mut buffer = slint::SharedPixelBuffer::<Rgb8Pixel>::new(8, 8);
    for byte in buffer.make_mut_bytes() {
        *byte = 90;
    }
    slint::Image::from_rgb8(buffer)
}

#[test]
fn song_menu_ignores_artwork_and_closes_for_another_song() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform(window.clone()))).unwrap();
    let ui = AppWindow::new().unwrap();
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.set_light_mode(false);
    ui.set_search_open(true);
    ui.set_search_rows(ModelRc::new(VecModel::from(vec![row(
        "aaaaaaaaaaa",
        "A Quiet Sun",
        slint::Image::default(),
    )])));
    let closed = draw(&window, 1140, 800);

    click(&ui, 1087.0, 421.0);
    let open = draw(&window, 1140, 800);
    let (changed, left, top, right, bottom) = changed_area(&closed, &open, 1140);
    assert!(changed > 0, "the song menu did not open");
    let menu_area = (left, top, right, bottom);
    assert!(
        right - left > 200 && bottom - top > 100,
        "the opened menu is not where the row's actions render: {menu_area:?}"
    );

    // The cover arrives: the row's struct changes, the song identity does not.
    ui.get_search_rows()
        .set_row_data(0, row("aaaaaaaaaaa", "A Quiet Sun", cover()));
    let after_artwork = draw(&window, 1140, 800);
    assert!(
        same_area(&open, &after_artwork, 1140, menu_area),
        "an artwork update closed the song menu"
    );

    // The row is recycled for a different song: the menu must not survive.
    ui.get_search_rows()
        .set_row_data(0, row("bbbbbbbbbbb", "Hollow Coast", cover()));
    let after_song = draw(&window, 1140, 800);
    assert!(
        same_area(&closed, &after_song, 1140, menu_area),
        "the song menu stayed open for a different song"
    );
}
