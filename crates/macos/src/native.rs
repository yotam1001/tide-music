//! AppKit resources are owned by the creating UI thread.
// Public raw-view signatures mirror the platform contract.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
use super::{MediaAction, MenuAction};
use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, Bool, Sel},
    sel, MainThreadMarker,
};
use objc2_app_kit as _;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_media_player as _;
use objc2_user_notifications as _;
use std::{cell::RefCell, ffi::c_void, path::Path, sync::mpsc};
type Obj = Retained<AnyObject>;
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "MediaPlayer", kind = "framework")]
unsafe extern "C" {}
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}
fn main_thread() -> Result<(), String> {
    MainThreadMarker::new()
        .map(|_| ())
        .ok_or_else(|| "AppKit integration requires the main thread".into())
}
fn string(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}
unsafe fn app() -> Obj {
    msg_send![class!(NSApplication), sharedApplication]
}
unsafe fn workspace() -> Obj {
    msg_send![class!(NSWorkspace), sharedWorkspace]
}
unsafe fn window(view: *mut c_void) -> Result<Obj, String> {
    main_thread()?;
    let view = (view as *mut AnyObject)
        .as_ref()
        .ok_or("Window view is missing")?;
    let window: Option<Obj> = msg_send![view, window];
    window.ok_or_else(|| "Window is not ready".into())
}
pub fn open_web_page(url: &str) -> Result<(), String> {
    main_thread()?;
    if !url.starts_with("https://")
        || url.len() > 2048
        || url.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("Only HTTPS web pages can be opened".into());
    }
    unsafe {
        let url: Option<Obj> = msg_send![class!(NSURL),URLWithString:&*string(url)];
        let url = url.ok_or("Invalid URL")?;
        let ok: Bool = msg_send![&*workspace(),openURL:&*url];
        if ok.as_bool() {
            Ok(())
        } else {
            Err("Could not open browser".into())
        }
    }
}
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    main_thread()?;
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    unsafe {
        let url: Obj = msg_send![class!(NSURL),fileURLWithPath:&*string(&path.to_string_lossy())];
        let urls: Obj = msg_send![class!(NSArray),arrayWithObject:&*url];
        let _: () = msg_send![&*workspace(),activateFileViewerSelectingURLs:&*urls];
    }
    Ok(())
}
pub fn app_light_theme() -> Option<bool> {
    main_thread().ok()?;
    unsafe {
        let appearance: Obj = msg_send![&*app(), effectiveAppearance];
        let a = string("NSAppearanceNameAqua");
        let d = string("NSAppearanceNameDarkAqua");
        let names: Obj = msg_send![class!(NSMutableArray), arrayWithObject: &*a];
        let _: () = msg_send![&*names, addObject: &*d];
        let best: Option<Retained<NSString>> =
            msg_send![&*appearance,bestMatchFromAppearancesWithNames:&*names];
        best.map(|s| s.to_string() == "NSAppearanceNameAqua")
    }
}
pub fn local_hour() -> Option<u8> {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            None
        } else {
            u8::try_from(tm.tm_hour).ok().filter(|h| *h < 24)
        }
    }
}
pub fn client_animations_enabled() -> bool {
    if main_thread().is_err() {
        return true;
    }
    unsafe {
        let reduced: Bool = msg_send![&*workspace(), accessibilityDisplayShouldReduceMotion];
        !reduced.as_bool()
    }
}
unsafe fn screen_geometry(w: &AnyObject) -> Result<(Obj, f64, f64), String> {
    let screen: Option<Obj> = msg_send![w, screen];
    let screen = screen.ok_or("Screen is not ready")?;
    let scale: f64 = msg_send![&*screen, backingScaleFactor];
    let screens: Obj = msg_send![class!(NSScreen), screens];
    let primary: Obj = msg_send![&*screens,objectAtIndex:0usize];
    let frame: NSRect = msg_send![&*primary, frame];
    Ok((screen, scale, frame.origin.y + frame.size.height))
}
pub fn window_work_area(view: *mut c_void) -> Result<[i32; 4], String> {
    unsafe {
        let w = window(view)?;
        let minimized: Bool = msg_send![&*w, isMiniaturized];
        let zoomed: Bool = msg_send![&*w, isZoomed];
        if minimized.as_bool() || zoomed.as_bool() {
            return Err("Window is minimized or maximized".into());
        }
        let (s, scale, top) = screen_geometry(&w)?;
        let f: NSRect = msg_send![&*s, visibleFrame];
        Ok([
            (f.origin.x * scale).round() as i32,
            ((top - f.origin.y - f.size.height) * scale).round() as i32,
            ((f.origin.x + f.size.width) * scale).round() as i32,
            ((top - f.origin.y) * scale).round() as i32,
        ])
    }
}
pub fn fit_window_to_work_area(view: *mut c_void, work: [i32; 4]) -> Result<(), String> {
    unsafe {
        let w = window(view)?;
        let (_, s, top) = screen_geometry(&w)?;
        let [l, t, r, b] = work;
        if r <= l || b <= t {
            return Err("Monitor work area is empty".into());
        }
        let zoomed: Bool = msg_send![&*w, isZoomed];
        if zoomed.as_bool() {
            return Ok(());
        }
        let f: NSRect = msg_send![&*w, frame];
        let width = f.size.width.min((r - l) as f64 / s);
        let height = f.size.height.min((b - t) as f64 / s);
        let x = f.origin.x.clamp(l as f64 / s, r as f64 / s - width);
        let y = f
            .origin
            .y
            .clamp(top - b as f64 / s, top - t as f64 / s - height);
        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
        let _: () = msg_send![&*w,setFrame:frame,display:Bool::YES];
        Ok(())
    }
}
pub mod mini {
    use super::*;
    #[derive(Default)]
    pub struct MiniWindow {
        original: Option<usize>,
    }
    impl MiniWindow {
        pub fn apply(
            &mut self,
            view: *mut c_void,
            pinned: bool,
            opacity: u8,
        ) -> Result<(), String> {
            unsafe {
                let w = window(view)?;
                let original = *self
                    .original
                    .get_or_insert_with(|| msg_send![&*w, collectionBehavior]);
                let _: () = msg_send![&*w,setLevel:if pinned{3isize}else{0isize}];
                let _: () =
                    msg_send![&*w,setCollectionBehavior:if pinned{original|1|256}else{original}];
                let _: () = msg_send![&*w,setAlphaValue:opacity as f64/255.0];
                Ok(())
            }
        }
    }
}
pub struct MediaSession {
    center: Obj,
    handlers: Vec<(Obj, Obj)>,
    info: Obj,
    sender: mpsc::Sender<MediaAction>,
    status: RefCell<Option<Obj>>,
    song: String,
    playing: bool,
}
pub fn start(
    view: *mut c_void,
    callback: impl Fn(MediaAction) + Send + 'static,
) -> Result<MediaSession, String> {
    unsafe {
        let _ = window(view)?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("tide-media".into())
            .spawn(move || {
                for action in rx {
                    let _ =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(action)));
                }
            })
            .map_err(|e| e.to_string())?;
        let center: Obj = msg_send![class!(MPRemoteCommandCenter), sharedCommandCenter];
        let info: Obj = msg_send![class!(NSMutableDictionary), new];
        let mut handlers = Vec::new();
        for (selector, action) in [
            (sel!(playCommand), MediaAction::Play),
            (sel!(pauseCommand), MediaAction::Pause),
            (sel!(togglePlayPauseCommand), MediaAction::Toggle),
            (sel!(nextTrackCommand), MediaAction::Next),
            (sel!(previousTrackCommand), MediaAction::Previous),
            (sel!(stopCommand), MediaAction::Stop),
            (sel!(changePlaybackPositionCommand), MediaAction::Seek(0.0)),
        ] {
            let command: Obj = objc2::msg_send![&*center,performSelector:selector];
            let sender = tx.clone();
            let block = RcBlock::new(move |event: *mut AnyObject| -> isize {
                let action = if matches!(action, MediaAction::Seek(_)) {
                    let position: f64 = msg_send![event, positionTime];
                    MediaAction::Seek(position)
                } else {
                    action
                };
                if sender.send(action).is_ok() {
                    0
                } else {
                    200
                }
            });
            let token: Obj = msg_send![&*command,addTargetWithHandler:&*block];
            let _: () = msg_send![&*command,setEnabled:Bool::YES];
            handlers.push((command, token));
        }
        Ok(MediaSession {
            center,
            handlers,
            info,
            sender: tx,
            status: RefCell::new(None),
            song: "Tide".into(),
            playing: false,
        })
    }
}
impl MediaSession {
    fn publish(&self) {
        unsafe {
            let center: Obj = msg_send![class!(MPNowPlayingInfoCenter), defaultCenter];
            let _: () = msg_send![&*center,setNowPlayingInfo:&*self.info];
        }
    }
    pub fn set_close_to_tray(&self, enabled: bool) {
        if main_thread().is_err() {
            return;
        }
        unsafe {
            let bar: Obj = msg_send![class!(NSStatusBar), systemStatusBar];
            if let Some(item) = self.status.borrow_mut().take() {
                let _: () = msg_send![&*bar,removeStatusItem:&*item];
            }
            if enabled {
                let item: Obj = msg_send![&*bar,statusItemWithLength:-1.0f64];
                let button: Obj = msg_send![&*item, button];
                let _: () = msg_send![&*button,setTitle:&*string("♫")];
                let menu = status_menu(&self.song, self.playing, self.sender.clone());
                let _: () = msg_send![&*item,setMenu:&*menu];
                *self.status.borrow_mut() = Some(item);
            }
        }
    }
    pub fn set_thumbnail(&mut self, png: Option<&[u8]>) -> Result<(), String> {
        main_thread()?;
        unsafe {
            let key = string("artwork");
            let _: () = msg_send![&*self.info,removeObjectForKey:&*key];
            if let Some(png) = png {
                if png.len() > 128 * 1024
                    || png.len() < 24
                    || &png[..8] != b"\x89PNG\r\n\x1a\n"
                    || u32::from_be_bytes(png[16..20].try_into().unwrap()) > 160
                    || u32::from_be_bytes(png[20..24].try_into().unwrap()) > 160
                {
                    return Err("Artwork must be a PNG of at most 128 KiB and 160 pixels".into());
                }
                let data: Obj = msg_send![class!(NSData),dataWithBytes:png.as_ptr().cast::<c_void>(),length:png.len()];
                let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
                let image: Option<Obj> = msg_send![image,initWithData:&*data];
                let image = image.ok_or("Invalid PNG artwork")?;
                let size: NSSize = msg_send![&*image, size];
                let block = RcBlock::new(move |_: NSSize| -> *mut AnyObject {
                    Retained::as_ptr(&image).cast_mut()
                });
                let art: Allocated<AnyObject> = msg_send![class!(MPMediaItemArtwork), alloc];
                let art: Obj = msg_send![art,initWithBoundsSize:size,requestHandler:&*block];
                let _: () = msg_send![&*self.info,setObject:&*art,forKey:&*key];
            }
            self.publish();
            Ok(())
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        title: &str,
        artist: &str,
        album: &str,
        playing: bool,
        position: f64,
        duration: f64,
    ) -> Result<(), String> {
        main_thread()?;
        unsafe {
            for (key, value) in [("title", title), ("artist", artist), ("albumTitle", album)] {
                let _: () = msg_send![&*self.info,setObject:&*string(value),forKey:&*string(key)];
            }
            for (key, value) in [
                (
                    "MPNowPlayingInfoPropertyElapsedPlaybackTime",
                    position.max(0.0),
                ),
                ("playbackDuration", duration.max(0.0)),
                (
                    "MPNowPlayingInfoPropertyPlaybackRate",
                    if playing { 1.0 } else { 0.0 },
                ),
            ] {
                let number: Obj = msg_send![class!(NSNumber),numberWithDouble:value];
                let _: () = msg_send![&*self.info,setObject:&*number,forKey:&*string(key)];
            }
            self.song = format!("{title} — {artist}").chars().take(72).collect();
            self.playing = playing;
            self.publish();
            if self.status.borrow().is_some() {
                self.set_close_to_tray(true);
            }
            Ok(())
        }
    }
}
impl Drop for MediaSession {
    fn drop(&mut self) {
        self.set_close_to_tray(false);
        unsafe {
            for (command, token) in &self.handlers {
                let _: () = msg_send![&**command,removeTarget:&**token];
            }
            let center: Obj = msg_send![class!(MPNowPlayingInfoCenter), defaultCenter];
            let _: () = msg_send![&*center,setNowPlayingInfo:std::ptr::null::<AnyObject>()];
        }
        let _ = &self.center;
    }
}
thread_local! {
    static MENU_CALLBACK:RefCell<Option<Box<dyn Fn(MenuAction)>>> = RefCell::new(None);
    static MENU_TARGET:RefCell<Option<Obj>> = const { RefCell::new(None) };
    static STATUS_CALLBACK:RefCell<Option<mpsc::Sender<MediaAction>>> = const { RefCell::new(None) };
    static DOCK_MENU:RefCell<Option<Obj>> = const { RefCell::new(None) };
}
unsafe extern "C" fn menu_action(_: *mut AnyObject, _: Sel, sender: *mut AnyObject) {
    let tag: isize = msg_send![sender, tag];
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if tag >= 100 {
            match tag {
                104 => {
                    let _: () = msg_send![&*app(),activateIgnoringOtherApps:Bool::YES];
                    MENU_CALLBACK.with_borrow(|c| {
                        if let Some(c) = c {
                            c(MenuAction::ShowWindow)
                        }
                    });
                }
                106 => {
                    let _: () = msg_send![&*app(),terminate:std::ptr::null::<AnyObject>()];
                }
                _ => {
                    let action = match tag {
                        100 => MediaAction::Toggle,
                        101 => MediaAction::Next,
                        102 => MediaAction::Previous,
                        105 => MediaAction::ShowMini,
                        _ => return,
                    };
                    STATUS_CALLBACK.with_borrow(|s| {
                        if let Some(s) = s {
                            let _ = s.send(action);
                        }
                    });
                }
            }
        } else {
            let action = match tag {
                0 => MenuAction::PlayPause,
                1 => MenuAction::Next,
                2 => MenuAction::Previous,
                3 => MenuAction::VolumeUp,
                4 => MenuAction::VolumeDown,
                5 => MenuAction::Like,
                6 => MenuAction::Search,
                7 => MenuAction::Settings,
                8 => MenuAction::ToggleMini,
                9 => MenuAction::ShowWindow,
                10 => MenuAction::HideWindow,
                _ => return,
            };
            MENU_CALLBACK.with_borrow(|c| {
                if let Some(c) = c {
                    c(action)
                }
            });
        }
    }));
}
unsafe fn target() -> Obj {
    MENU_TARGET.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| {
            let mut builder =
                objc2::runtime::ClassBuilder::new(c"TideMenuTarget", class!(NSObject))
                    .expect("unique Tide menu class");
            builder.add_method(
                sel!(tideAction:),
                menu_action as unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
            );
            let class = builder.register();
            msg_send![class, new]
        })
        .clone()
    })
}
unsafe fn menu() -> Obj {
    let m: Obj = msg_send![class!(NSMenu), new];
    let _: () = msg_send![&*m,setAutoenablesItems:Bool::NO];
    m
}
unsafe fn item(
    menu: &AnyObject,
    title: &str,
    key: &str,
    mask: usize,
    tag: isize,
    selector: Option<Sel>,
) {
    let item: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
    let item: Obj = msg_send![item,initWithTitle:&*string(title),action:selector.unwrap_or(sel!(tideAction:)),keyEquivalent:&*string(key)];
    let _: () = msg_send![&*item,setKeyEquivalentModifierMask:mask];
    let _: () = msg_send![&*item,setTag:tag];
    if selector.is_none() {
        let _: () = msg_send![&*item,setTarget:&*target()];
    }
    let _: () = msg_send![menu,addItem:&*item];
}
unsafe fn status_menu(song: &str, playing: bool, sender: mpsc::Sender<MediaAction>) -> Obj {
    STATUS_CALLBACK.with_borrow_mut(|s| *s = Some(sender));
    let m = menu();
    item(&m, song, "", 0, -1, None);
    let first: Obj = msg_send![&*m,itemAtIndex:0isize];
    let _: () = msg_send![&*first,setEnabled:Bool::NO];
    for (title, tag) in [
        (if playing { "Pause" } else { "Play" }, 100),
        ("Next", 101),
        ("Previous", 102),
        ("Show Tide", 104),
        ("Mini player", 105),
        ("Quit", 106),
    ] {
        item(&m, title, "", 0, tag, None);
    }
    m
}
unsafe extern "C" fn dock_menu(_: &AnyObject, _: Sel, _: *mut AnyObject) -> *mut AnyObject {
    DOCK_MENU.with_borrow(|m| {
        m.as_ref()
            .map_or(std::ptr::null_mut(), |m| Retained::as_ptr(m).cast_mut())
    })
}
#[link(name = "objc")]
unsafe extern "C" {
    fn class_addMethod(
        class: *const AnyClass,
        name: Sel,
        imp: unsafe extern "C" fn(),
        types: *const std::ffi::c_char,
    ) -> Bool;
}
pub fn install_app_menu(callback: impl Fn(MenuAction) + 'static) -> Result<(), String> {
    main_thread()?;
    unsafe {
        MENU_CALLBACK.with_borrow_mut(|c| *c = Some(Box::new(callback)));
        let root = menu();
        let cmd = 1usize << 20;
        let shift = 1usize << 17;
        for title in ["Tide", "Edit", "Controls", "Window", "Help"] {
            let submenu = menu();
            let _: () = msg_send![&*submenu,setTitle:&*string(title)];
            match title {
                "Tide" => {
                    item(&submenu, "Settings…", ",", cmd, 7, None);
                    item(&submenu, "Quit Tide", "q", cmd, -1, Some(sel!(terminate:)));
                }
                "Edit" => {
                    for (t, k, s) in [
                        ("Cut", "x", sel!(cut:)),
                        ("Copy", "c", sel!(copy:)),
                        ("Paste", "v", sel!(paste:)),
                        ("Select All", "a", sel!(selectAll:)),
                    ] {
                        item(&submenu, t, k, cmd, -1, Some(s));
                    }
                    item(&submenu, "Search", "f", cmd, 6, None);
                }
                "Controls" => {
                    for (t, k, tag) in [
                        ("Play/Pause", "", 0),
                        ("Next", "\u{f703}", 1),
                        ("Previous", "\u{f702}", 2),
                        ("Volume Up", "\u{f700}", 3),
                        ("Volume Down", "\u{f701}", 4),
                        ("Like", "l", 5),
                    ] {
                        item(&submenu, t, k, cmd, tag, None);
                    }
                    item(&submenu, "Mini player", "m", cmd | shift, 8, None);
                }
                "Window" => {
                    item(&submenu, "Hide window", "w", cmd, 10, None);
                    item(
                        &submenu,
                        "Minimize",
                        "m",
                        cmd,
                        -1,
                        Some(sel!(performMiniaturize:)),
                    );
                    item(&submenu, "Show Tide", "", 0, 9, None);
                }
                _ => {
                    item(&submenu, "Tide Help", "", 0, -1, Some(sel!(showHelp:)));
                }
            }
            let parent: Obj = msg_send![class!(NSMenuItem), new];
            let _: () = msg_send![&*parent,setTitle:&*string(title)];
            let _: () = msg_send![&*parent,setSubmenu:&*submenu];
            let _: () = msg_send![&*root,addItem:&*parent];
        }
        let _: () = msg_send![&*app(),setMainMenu:&*root];
        let dock = menu();
        for (title, tag) in [
            ("Play/Pause", 0),
            ("Next", 1),
            ("Previous", 2),
            ("Like", 5),
            ("Mini player", 8),
        ] {
            item(&dock, title, "", 0, tag, None);
        }
        DOCK_MENU.with_borrow_mut(|m| *m = Some(dock));
        let delegate: Option<Obj> = msg_send![&*app(), delegate];
        let delegate = delegate.ok_or("Application delegate is not ready")?;
        let class = delegate.class();
        let added = class_addMethod(
            class,
            sel!(applicationDockMenu:),
            std::mem::transmute::<
                unsafe extern "C" fn(&AnyObject, Sel, *mut AnyObject) -> *mut AnyObject,
                unsafe extern "C" fn(),
            >(dock_menu),
            c"@@:@".as_ptr(),
        );
        if !added.as_bool() {
            return Err("Could not install Dock menu".into());
        }
        Ok(())
    }
}
pub fn notify_song(title: &str, artist: &str) -> Result<(), String> {
    main_thread()?;
    unsafe {
        let bundle: Obj = msg_send![class!(NSBundle), mainBundle];
        let id: Option<Retained<NSString>> = msg_send![&*bundle, bundleIdentifier];
        if id.is_none() {
            return Err("Notifications require a bundled macOS application".into());
        }
        let center: Obj = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let content: Obj = msg_send![class!(UNMutableNotificationContent), new];
        let _: () = msg_send![&*content,setTitle:&*string(title)];
        let _: () = msg_send![&*content,setBody:&*string(artist)];
        let request: Obj = msg_send![class!(UNNotificationRequest),requestWithIdentifier:&*string("tide-song"),content:&*content,trigger:std::ptr::null::<AnyObject>()];
        let block = RcBlock::new(move |granted: Bool, _error: *mut AnyObject| {
            if granted.as_bool() {
                let center: Obj =
                    msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
                let completion = RcBlock::new(|_error: *mut AnyObject| {});
                let _: () = msg_send![&*center,addNotificationRequest:&*request,withCompletionHandler:&*completion];
            }
        });
        let _: () =
            msg_send![&*center,requestAuthorizationWithOptions:6usize,completionHandler:&*block];
        Ok(())
    }
}
mod audio;
pub use audio::OutputWatcher;
