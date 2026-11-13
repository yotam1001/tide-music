//! Opt-in `--smoke-scroll-stress N`: scroll a synthetic N-track playlist with
//! the production refresh loop running while thumbnails complete. It uses no
//! network, account or user data and reports counts/timings only.
use super::*;
use slint::platform::WindowEvent;

// `STRESS_TEXT=mixed` uses the awkward metadata generated lists carry: many
// scripts, emoji, newlines, empty fields and very long names.
const TEXTS: &[&str] = &[
    "Plain title",
    "तेरे बिना (From \"Aashiqui\") - हिन्दी",
    "வணக்கம் தமிழ் பாடல்",
    "أغنية عربية – مرحبا",
    "שיר בעברית",
    "夜に駆ける 🌙",
    "봄날 ✨",
    "สวัสดีครับ",
    "Emoji 🎵🎶👩‍👩‍👧‍👦🏳️‍🌈🇮🇳 🔥🔥🔥",
    "Line one\nLine two\nLine three",
    "Tab\tand\r\ncarriage return",
    "z̷̢̛͓a̸̧͙l̶̼̈g̷͇̾o̴̖͝",
    "\u{200f}RTL\u{200e} \u{202e}override\u{202c}",
    "A very long name that keeps going without a natural break so the row must elide it: loremipsumdolorsitametconsecteturadipiscingelitseddoeiusmodtemporincididunt",
    "",
    "   ",
];

fn tracks(count: usize) -> Vec<Track> {
    let mixed = std::env::var("STRESS_TEXT").is_ok_and(|v| v == "mixed");
    let text = |i: usize, fallback: String| {
        if mixed {
            TEXTS[i % TEXTS.len()].to_owned()
        } else {
            fallback
        }
    };
    (0..count)
        .map(|i| Track {
            artists: Vec::new(),
            title: text(i, format!("Synthetic song {i}")),
            artist: text(i * 7 + 3, format!("Synthetic artist {}", i % 97)),
            album: text(i * 5 + 1, format!("Synthetic album {}", i % 31)),
            source: format!("fixture-{i}"),
            duration_seconds: (!mixed || i % 5 != 0).then_some(120 + (i as u64 % 200)),
            artwork_url: Some(format!("https://i.ytimg.com/vi/stress{i:06}/hq.jpg")),
            recommended: false,
        })
        .collect()
}

pub(super) fn start(
    state: &Rc<RefCell<State>>,
    ui: &AppWindow,
    count: usize,
    feed: artwork::SyntheticFeed,
) -> slint::Timer {
    let timer = slint::Timer::default();
    let state = state.clone();
    let weak = ui.as_weak();
    let run = Duration::from_secs(
        std::env::var("STRESS_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20),
    );
    let began = Instant::now();
    let mut ready = false;
    let mut last_tick = Instant::now();
    let (mut ticks, mut stalls, mut worst) = (0_u64, 0_u64, Duration::ZERO);
    let mut seed = 0x9e37_79b9_u32;
    let mut direction = -1.0_f32;
    let (mut deepest, mut turns) = (0.0_f32, 0_u32);
    // `STRESS_PATTERN=edge` wheels back and forth across the header-collapse
    // threshold near the top instead of sweeping the whole list.
    let edge = std::env::var("STRESS_PATTERN").is_ok_and(|p| p == "edge");
    let (mut last_view, mut view_changes) = (None::<f32>, 0_u32);
    let jump_every: u64 = std::env::var("STRESS_JUMP_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(40);
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(u) = weak.upgrade() else { return };
        if !ready {
            {
                let mut s = state.borrow_mut();
                s.detail_tracks = tracks(count);
                u.set_detail_title("Synthetic playlist".into());
                // Liked Songs opens without a cover; My Supermix and playlists
                // with one. `STRESS_VARIANT=liked|supermix` mirrors those views.
                let variant = std::env::var("STRESS_VARIANT").unwrap_or_default();
                u.set_detail_has_cover(variant != "liked");
                // `nav-only` and `recommended` split the Supermix variant.
                let navigation = match variant.as_str() {
                    "liked" | "supermix" | "nav-only" => variant.trim_end_matches("-only"),
                    _ => "",
                };
                u.set_detail_navigation(navigation.replace("nav", "supermix").into());
                for t in s.detail_tracks.iter_mut() {
                    t.recommended = matches!(variant.as_str(), "supermix" | "recommended");
                }
                u.set_detail_open(true);
                u.set_library_open(false);
                refresh(&u, &s);
                refresh_content_models(&s, &u);
            }
            u.window().set_size(slint::LogicalSize::new(1200.0, 800.0));
            ready = true;
            last_tick = Instant::now();
            return;
        }
        let gap = last_tick.elapsed();
        last_tick = Instant::now();
        worst = worst.max(gap);
        stalls += u64::from(gap > Duration::from_millis(250));
        ticks += 1;
        let row = u.get_detail_row_height().max(1.0);
        let view = u.get_detail_artwork_height();
        view_changes += u32::from(last_view.is_some_and(|last| last != view));
        last_view = Some(view);
        let max_y = (count as f32 * row - view).max(0.0);
        let y = -u.get_detail_artwork_y();
        deepest = deepest.max(y);
        // Wheel scrolling, bouncing between both ends.
        if edge {
            if y < 8.0 {
                direction = -1.0;
            } else if y > 44.0 {
                direction = 1.0;
            }
        } else if (y >= max_y - 1.0 && direction < 0.0) || (y <= 1.0 && direction > 0.0) {
            direction = -direction;
            turns += 1;
        }
        u.window().dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(700.0, 680.0),
            delta_x: 0.0,
            delta_y: direction * if edge { 12.0 } else { 90.0 },
        });
        // The pointer stays over the rows while scrolling, as with a real mouse.
        for step in 0..3 {
            let x = 300.0 + ((ticks * 37 + step * 113) % 800) as f32;
            let y = 120.0 + ((ticks * 53 + step * 71) % 640) as f32;
            u.window().dispatch_event(WindowEvent::PointerMoved {
                position: slint::LogicalPosition::new(x, y),
            });
        }
        // Scrollbar-thumb jumps to arbitrary positions.
        if !edge && ticks % jump_every == 0 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let at = (seed as f32 / u32::MAX as f32) * max_y;
            u.set_detail_artwork_y(-at);
        }
        // Thumbnails finish for the rows now in view.
        let first = (y / row) as usize;
        for k in 0..3 {
            let index = (first + k * 3 + ticks as usize % 3).min(count.saturating_sub(1));
            feed.push(
                &format!("https://i.ytimg.com/vi/stress{index:06}/hq.jpg"),
                (index % 200) as u8 + 30,
            );
        }
        if began.elapsed() >= run {
            println!(
                "PASS scroll stress: {count} tracks, {ticks} ticks, {stalls} stalls over 250 ms, worst gap {} ms, deepest {deepest:.0}/{max_y:.0} px, {turns} direction changes, {view_changes} viewport height changes",
                worst.as_millis()
            );
            let _ = slint::quit_event_loop();
        }
    });
    timer
}
