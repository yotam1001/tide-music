//! Native thumbnail transport. All ownership and callbacks stay on the UI thread.
//! No extra process, window, network connection or polling thread is created.
use crate::MediaAction;
use std::{cell::Cell, marker::PhantomData, rc::Rc};
use windows::Win32::{
    Foundation::POINT,
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            AppendMenuW, CopyIcon, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW,
            SendMessageW, SetForegroundWindow, ShowWindow, TrackPopupMenu, HICON as TrayIcon,
            ICON_SMALL, MF_STRING, SC_CLOSE, SW_HIDE, SW_MINIMIZE, SW_RESTORE, TPM_NONOTIFY,
            TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_GETICON, WM_LBUTTONUP,
            WM_RBUTTONUP, WM_SYSCOMMAND,
        },
    },
};
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::{
            Shell::{
                DefSubclassProc, ITaskbarList3, RemoveWindowSubclass, SetWindowSubclass,
                TaskbarList, THBF_DISABLED, THBF_ENABLED, THBN_CLICKED, THB_FLAGS, THB_ICON,
                THB_TOOLTIP, THUMBBUTTON,
            },
            WindowsAndMessaging::{
                CreateIcon, DestroyIcon, IsWindow, RegisterWindowMessageW, HICON, WM_COMMAND,
                WM_NCDESTROY,
            },
        },
    },
};
const TRAY_MESSAGE: u32 = WM_APP + 73;
const SHOW: u32 = 0x7201;
const HIDE: u32 = 0x7202;
const QUIT: u32 = 0x7203;
const MINI: u32 = 0x7204;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE;
fn light_theme_setting(name: windows::core::PCWSTR) -> Option<bool> {
    let mut value = 0u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    }
    .ok()
    .map(|()| value != 0)
}
fn system_light_theme() -> bool {
    light_theme_setting(w!("SystemUsesLightTheme")).unwrap_or(false)
}

pub(super) fn app_light_theme() -> Option<bool> {
    light_theme_setting(w!("AppsUseLightTheme"))
}
const SUBCLASS: usize = 0x5449_4445;
const PREVIOUS: u32 = 0x7101;
const TOGGLE: u32 = 0x7102;
const NEXT: u32 = 0x7103;
struct Icon(HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}
struct State {
    list: ITaskbarList3,
    icons: Vec<Icon>,
    callback: Box<dyn Fn(MediaAction)>,
    message: u32,
    alive: Cell<bool>,
    added: Cell<bool>,
    attempts: Cell<u8>,
    playing: Cell<bool>,
    light_theme: Cell<bool>,
    enabled: Cell<bool>,
    tray_allowed: bool,
    close_to_tray: Cell<bool>,
    tray_added: Cell<bool>,
    tray_created: u32,
}
impl State {
    fn publish(&self, hwnd: HWND) {
        if !self.alive.get() || (!self.added.get() && self.attempts.get() >= 3) {
            return;
        }
        let labels = [
            "Previous song",
            if self.playing.get() { "Pause" } else { "Play" },
            "Next song",
        ];
        let mut buttons = [THUMBBUTTON::default(); 3];
        for (i, button) in buttons.iter_mut().enumerate() {
            button.dwMask = THB_FLAGS | THB_ICON | THB_TOOLTIP;
            button.iId = [PREVIOUS, TOGGLE, NEXT][i];
            button.hIcon = self.icons[(if self.light_theme.get() { 4 } else { 0 })
                + (if i == 1 && self.playing.get() { 3 } else { i })]
            .0;
            button.dwFlags = if self.enabled.get() {
                THBF_ENABLED
            } else {
                THBF_DISABLED
            };
            for (slot, code) in button.szTip.iter_mut().zip(labels[i].encode_utf16()) {
                *slot = code;
            }
        }
        if self.tray_allowed && !self.tray_added.get() {
            self.add_tray(hwnd);
        }
        // Explorer copies the small icon bitmaps; they remain owned here until detach.
        unsafe {
            if self.added.get() {
                let _ = self.list.ThumbBarUpdateButtons(hwnd, &buttons);
            } else {
                self.attempts.set(self.attempts.get() + 1);
                self.added
                    .set(self.list.ThumbBarAddButtons(hwnd, &buttons).is_ok());
            }
        }
    }
}
impl State {
    fn tray_data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        }
    }
    fn add_tray(&self, hwnd: HWND) {
        let mut data = self.tray_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon = self.icons.last().unwrap().0;
        for (slot, code) in data
            .szTip
            .iter_mut()
            .zip("Tide — click to open".encode_utf16())
        {
            *slot = code;
        }
        self.tray_added
            .set(unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool());
    }
    fn remove_tray(&self, hwnd: HWND) {
        if self.tray_added.replace(false) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &self.tray_data(hwnd));
            }
        }
    }
    fn show_player(&self, hwnd: HWND) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
    fn show_menu(&self, hwnd: HWND) {
        // Menu commands are fixed constants; no paths, URLs, scripts or credentials enter them.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            for (id, label) in [
                (SHOW, "Open Tide"),
                (MINI, "Open Mini player"),
                (TOGGLE, if self.playing.get() { "Pause" } else { "Play" }),
                (PREVIOUS, "Previous song"),
                (NEXT, "Next song"),
                (HIDE, "Minimize to tray"),
                (QUIT, "Quit Tide"),
            ] {
                let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                let _ = AppendMenuW(
                    menu,
                    MF_STRING,
                    id as usize,
                    windows::core::PCWSTR(text.as_ptr()),
                );
            }
            let mut point = POINT::default();
            if GetCursorPos(&mut point).is_err() {
                let _ = DestroyMenu(menu);
                return;
            }
            let _ = SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                hwnd,
                None,
            )
            .0 as u32;
            let _ = DestroyMenu(menu);
            match command {
                SHOW => self.show_player(hwnd),
                MINI => {
                    self.show_player(hwnd);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (self.callback)(MediaAction::ShowMini)
                    }));
                }
                HIDE => {
                    let _ = ShowWindow(hwnd, SW_MINIMIZE);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                QUIT => {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                TOGGLE | PREVIOUS | NEXT if self.enabled.get() => {
                    let action = match command {
                        PREVIOUS => MediaAction::Previous,
                        NEXT => MediaAction::Next,
                        _ => MediaAction::Toggle,
                    };
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (self.callback)(action)
                    }));
                }
                _ => {}
            }
        }
    }
}
pub(super) struct Taskbar {
    hwnd: HWND,
    state: Option<Rc<State>>,
    _ui_thread: PhantomData<Rc<()>>,
}
impl Taskbar {
    /// The caller holds a COM apartment for this object's entire lifetime.
    pub(super) fn attach(
        hwnd: HWND,
        callback: impl Fn(MediaAction) + 'static,
        tray_allowed: bool,
    ) -> Result<Self, String> {
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err("Taskbar window unavailable".into());
        }
        let message = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
        if message == 0 {
            return Err("Taskbar message unavailable".into());
        }
        let list: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| "Windows taskbar unavailable")?;
        unsafe { list.HrInit() }.map_err(|_| "Windows taskbar unavailable")?;
        let mut icons = Vec::with_capacity(9);
        for kind in 0..8 {
            let (mask, pixels) = icon_pixels(kind % 4, kind >= 4);
            icons.push(Icon(
                unsafe { CreateIcon(None, 16, 16, 1, 32, mask.as_ptr(), pixels.as_ptr()) }
                    .map_err(|_| "Taskbar icon unavailable")?,
            ));
        }
        // Copy the actual window icon. A fallback remains owned if Windows has not set it yet.
        let mut raw = unsafe {
            SendMessageW(
                hwnd,
                WM_GETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(0)),
            )
        };
        if raw.0 == 0 {
            raw = unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(1)), Some(LPARAM(0))) };
        }
        if raw.0 != 0 {
            if let Ok(icon) = unsafe { CopyIcon(TrayIcon(raw.0 as *mut _)) } {
                icons.push(Icon(icon));
            }
        }
        let has_tray_icon = icons.len() > 8;
        let state = Rc::new(State {
            list,
            icons,
            callback: Box::new(callback),
            message,
            alive: Cell::new(true),
            added: Cell::new(false),
            attempts: Cell::new(0),
            playing: Cell::new(false),
            light_theme: Cell::new(system_light_theme()),
            enabled: Cell::new(false),
            tray_allowed: tray_allowed && has_tray_icon,
            close_to_tray: Cell::new(false),
            tray_added: Cell::new(false),
            tray_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
        });
        // The Rc address is stable. Each dispatched message retains it across reentrant calls.
        if !unsafe {
            SetWindowSubclass(hwnd, Some(procedure), SUBCLASS, Rc::as_ptr(&state) as usize)
        }
        .as_bool()
        {
            return Err("Could not attach taskbar controls".into());
        }
        Ok(Self {
            hwnd,
            state: Some(state),
            _ui_thread: PhantomData,
        })
    }
    pub(super) fn set_close_to_tray(&self, enabled: bool) {
        if let Some(state) = &self.state {
            state.close_to_tray.set(enabled);
        }
    }

    pub(super) fn update(&self, playing: bool, enabled: bool) {
        if let Some(state) = &self.state {
            let playing_changed = state.playing.replace(playing) != playing;
            let enabled_changed = state.enabled.replace(enabled) != enabled;
            if playing_changed || enabled_changed || !state.added.get() {
                state.publish(self.hwnd);
            }
        }
    }
}
impl Drop for Taskbar {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            state.remove_tray(self.hwnd);
            if state.alive.get()
                && !unsafe { RemoveWindowSubclass(self.hwnd, Some(procedure), SUBCLASS) }.as_bool()
            {
                // An unexpected detach failure must never leave a dangling window pointer.
                // MediaSession deactivates the shared callback before this guard drops.
                let _ = Rc::into_raw(state);
            }
        }
    }
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // A menu or a playback callback can pump messages and detach the owner.
    // Retain state until this invocation returns, including through DefSubclassProc.
    let state = unsafe {
        let pointer = data as *const State;
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    if message == WM_NCDESTROY {
        state.remove_tray(hwnd);
        state.alive.set(false);
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS);
        }
    } else if message == WM_SYSCOMMAND
        && (wp.0 & 0xfff0) as u32 == SC_CLOSE
        && state.close_to_tray.get()
        && state.tray_added.get()
    {
        // System-menu close is a user gesture. Leave WM_CLOSE to explicit
        // Quit and installer shutdown so updating still closes the app.
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        return LRESULT(0);
    } else if message == WM_SETTINGCHANGE {
        let light = system_light_theme();
        if state.light_theme.replace(light) != light {
            state.publish(hwnd);
        }
    } else if message == TRAY_MESSAGE {
        if lp.0 as u32 == WM_LBUTTONUP {
            state.show_player(hwnd);
        } else if lp.0 as u32 == WM_RBUTTONUP {
            state.show_menu(hwnd);
        }
        return LRESULT(0);
    } else if state.tray_created != 0 && message == state.tray_created {
        state.tray_added.set(false);
        if state.tray_allowed {
            state.add_tray(hwnd);
        }
    } else if message == state.message {
        state.added.set(false);
        state.attempts.set(0);
        state.publish(hwnd);
    } else if message == WM_COMMAND
        && lp.0 == 0
        && ((wp.0 >> 16) & 0xffff) as u32 == THBN_CLICKED
        && state.enabled.get()
    {
        let action = match (wp.0 & 0xffff) as u32 {
            PREVIOUS => Some(MediaAction::Previous),
            TOGGLE => Some(MediaAction::Toggle),
            NEXT => Some(MediaAction::Next),
            _ => None,
        };
        if let Some(action) = action {
            // Neither a Rust panic nor unwinding may cross a Windows callback boundary.
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.callback)(action)));
            return LRESULT(0);
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wp, lp) }
}
fn icon_pixels(kind: usize, light: bool) -> ([u8; 32], [u8; 1024]) {
    let mut mask = [255u8; 32];
    let mut pixels = [0u8; 1024];
    for y in 0usize..16 {
        for x in 0usize..16 {
            let middle = (y as i32 - 7).abs();
            let on = match kind {
                0 => {
                    (x == 3 || x == 4) && (3..13).contains(&y)
                        || (5..12).contains(&x) && middle <= (x as i32 - 5) * 4 / 6
                }
                1 => (5..12).contains(&x) && middle <= (11 - x) as i32 * 4 / 6,
                2 => {
                    (x == 11 || x == 12) && (3..13).contains(&y)
                        || (4..11).contains(&x) && middle <= (10 - x) as i32 * 4 / 6
                }
                _ => (x == 5 || x == 6 || x == 10 || x == 11) && (3..13).contains(&y),
            };
            if on {
                mask[y * 2 + x / 8] &= !(0x80 >> (x % 8));
                pixels[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(if light {
                    &[45, 40, 45, 255]
                } else {
                    &[235, 235, 240, 255]
                });
            }
        }
    }
    (mask, pixels)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_can_detach_its_own_controls() {
        use std::cell::RefCell;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        };
        let _apartment = crate::native::Apartment::initialize().unwrap();
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Tide reentrant fixture"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let owner = Rc::new(RefCell::new(None::<Taskbar>));
        let weak_owner = Rc::downgrade(&owner);
        let fired = Rc::new(Cell::new(0));
        let count = fired.clone();
        let controls = Taskbar::attach(
            hwnd,
            move |_| {
                count.set(count.get() + 1);
                drop(weak_owner.upgrade().unwrap().borrow_mut().take());
            },
            false,
        )
        .unwrap();
        controls.update(true, true);
        *owner.borrow_mut() = Some(controls);
        for _ in 0..2 {
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_COMMAND,
                    Some(WPARAM(((THBN_CLICKED as usize) << 16) | TOGGLE as usize)),
                    Some(LPARAM(0)),
                );
            }
        }
        assert_eq!(
            fired.get(),
            1,
            "detachment must remove the callback immediately"
        );
        assert!(owner.borrow().is_none());
        unsafe {
            DestroyWindow(hwnd).unwrap();
        }
    }
    #[test]
    fn thumbnail_clicks_use_the_media_route_and_detach_on_drop() {
        use std::sync::{Arc, Mutex};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        };
        let _apartment = crate::native::Apartment::initialize().unwrap();
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Tide taskbar fixture"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let sink = actions.clone();
        let taskbar = Taskbar::attach(hwnd, move |a| sink.lock().unwrap().push(a), false).unwrap();
        taskbar.update(true, true);
        for id in [PREVIOUS, TOGGLE, NEXT] {
            unsafe {
                SendMessageW(
                    hwnd,
                    WM_COMMAND,
                    Some(WPARAM(((THBN_CLICKED as usize) << 16) | id as usize)),
                    Some(LPARAM(0)),
                );
            }
        }
        assert_eq!(
            *actions.lock().unwrap(),
            [
                MediaAction::Previous,
                MediaAction::Toggle,
                MediaAction::Next
            ]
        );
        taskbar.update(false, false);
        unsafe {
            SendMessageW(
                hwnd,
                WM_COMMAND,
                Some(WPARAM(((THBN_CLICKED as usize) << 16) | TOGGLE as usize)),
                Some(LPARAM(0)),
            );
        }
        assert_eq!(actions.lock().unwrap().len(), 3);
        drop(taskbar);
        unsafe {
            SendMessageW(
                hwnd,
                WM_COMMAND,
                Some(WPARAM(((THBN_CLICKED as usize) << 16) | TOGGLE as usize)),
                Some(LPARAM(0)),
            );
            DestroyWindow(hwnd).unwrap();
        }
        assert_eq!(actions.lock().unwrap().len(), 3);
    }
}

#[cfg(test)]
mod resource_probe {
    use super::*;
    #[test]
    #[ignore = "Run this native resource probe alone with --test-threads=1"]
    fn taskbar_resource_cycles_release_icons_and_subclasses() {
        use windows::Win32::{
            System::Threading::{
                GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS,
            },
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
            },
        };
        let _apartment = crate::native::Apartment::initialize().unwrap();
        let cycle = || unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Tide lifecycle fixture"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                100,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let controls = Taskbar::attach(hwnd, |_| {}, false).unwrap();
            for i in 0..10 {
                controls.update(i % 2 == 0, true);
            }
            drop(controls);
            DestroyWindow(hwnd).unwrap();
        };
        cycle();
        let count = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS),
                GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS),
            )
        };
        let before = count();
        for _ in 0..40 {
            cycle();
        }
        let after = count();
        assert!(
            after.0 <= before.0 + 2 && after.1 <= before.1 + 2,
            "Native transport resources grew: {before:?} -> {after:?}"
        );
        println!(
            "PASS 40 native taskbar attach/update/drop cycles: USER/GDI {before:?} -> {after:?}"
        );
    }
}
