//! Settings > About links. Each one only opens a public page in the browser
//! when clicked; nothing is fetched in the background.
use super::*;

const REPOSITORY: &str = "https://github.com/yotam1001/tide-music";

/// Matches the `open-about-link` indexes in `ui/main.slint`. Documents are
/// pinned to the running version's tag, so they describe this build.
fn about_link(index: i32, version: &str) -> Option<String> {
    Some(match index {
        0 => format!("{REPOSITORY}/releases/tag/v{version}"),
        1 => format!("{REPOSITORY}/issues/new"),
        2 => REPOSITORY.into(),
        3 => format!("{REPOSITORY}/blob/v{version}/PRIVACY.md"),
        4 => format!("{REPOSITORY}/blob/v{version}/THIRD_PARTY_NOTICES.md"),
        _ => return None,
    })
}

pub fn install(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_open_about_link(move |index| {
        let Some(url) = about_link(index, env!("CARGO_PKG_VERSION")) else {
            return;
        };
        if let (Err(error), Some(u)) = (crate::platform::open_web_page(&url), weak.upgrade()) {
            u.set_status_text(error.into());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_links_open_this_versions_pages() {
        assert_eq!(
            about_link(0, "1.2.3").as_deref(),
            Some("https://github.com/yotam1001/tide-music/releases/tag/v1.2.3")
        );
        assert_eq!(
            about_link(4, "1.2.3").as_deref(),
            Some("https://github.com/yotam1001/tide-music/blob/v1.2.3/THIRD_PARTY_NOTICES.md")
        );
        for index in 0..5 {
            assert!(about_link(index, "1.2.3").unwrap().starts_with("https://"));
        }
        assert!(about_link(5, "1.2.3").is_none());
        assert!(about_link(-1, "1.2.3").is_none());
    }
}
