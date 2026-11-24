//! One shared X11 connection for window management requests made from the UI
//! thread (Mini pinning, opacity, work area) and the tray thread (show/hide).
use std::ffi::c_void;
use std::sync::{Arc, OnceLock};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub(crate) struct X11 {
    pub conn: RustConnection,
    pub root: u32,
    atoms: Atoms,
}

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_WINDOW_OPACITY,
        _NET_WORKAREA,
        _NET_CURRENT_DESKTOP,
        _NET_FRAME_EXTENTS,
        _NET_ACTIVE_WINDOW,
        _NET_MOVERESIZE_WINDOW,
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        CARDINAL,
    }
}

const NOT_X11: &str = "Tide's window is not on an X11 display";

/// The process-wide connection, opened on first use.
pub(crate) fn shared() -> Result<Arc<X11>, String> {
    static SHARED: OnceLock<Result<Arc<X11>, String>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            let (conn, screen) = x11rb::connect(None).map_err(|_| NOT_X11.to_string())?;
            let root = conn.setup().roots[screen].root;
            let atoms = Atoms::new(&conn)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(X11 { conn, root, atoms }))
        })
        .clone()
}

/// `native_view` encodes the X11 window id as the pointer value.
pub(crate) fn window(view: *mut c_void) -> Result<u32, String> {
    u32::try_from(view as usize)
        .ok()
        .filter(|id| *id != 0)
        .ok_or_else(|| NOT_X11.to_string())
}

impl X11 {
    fn root_message(&self, window: u32, kind: Atom, data: [u32; 5]) -> Result<(), String> {
        let event = ClientMessageEvent::new(32, window, kind, data);
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    fn cardinals(&self, window: u32, property: Atom) -> Vec<u32> {
        self.conn
            .get_property(false, window, property, AtomEnum::ANY, 0, 1024)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default()
    }

    fn states(&self, window: u32) -> Vec<u32> {
        self.cardinals(window, self.atoms._NET_WM_STATE)
    }

    pub fn set_above(&self, window: u32, above: bool) -> Result<(), String> {
        // _NET_WM_STATE: action (0 remove, 1 add), property, 0, source 1 (application).
        self.root_message(
            window,
            self.atoms._NET_WM_STATE,
            [above as u32, self.atoms._NET_WM_STATE_ABOVE, 0, 1, 0],
        )
    }

    pub fn set_opacity(&self, window: u32, opacity: u8) -> Result<(), String> {
        if opacity == 255 {
            self.conn
                .delete_property(window, self.atoms._NET_WM_WINDOW_OPACITY)
                .map_err(|e| e.to_string())?;
        } else {
            let value = (u64::from(opacity) * u64::from(u32::MAX) / 255) as u32;
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms._NET_WM_WINDOW_OPACITY,
                    self.atoms.CARDINAL,
                    &[value],
                )
                .map_err(|e| e.to_string())?;
        }
        self.conn.flush().map_err(|e| e.to_string())
    }

    /// Unmap and withdraw (ICCCM 4.1.4), the X11 counterpart of `SW_HIDE`.
    pub fn hide(&self, window: u32) -> Result<(), String> {
        self.conn.unmap_window(window).map_err(|e| e.to_string())?;
        let event = x11rb::protocol::xproto::UnmapNotifyEvent {
            response_type: x11rb::protocol::xproto::UNMAP_NOTIFY_EVENT,
            sequence: 0,
            event: self.root,
            window,
            from_configure: false,
        };
        self.conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    /// Map, restore and activate, the counterpart of `SW_RESTORE` plus focus.
    pub fn show(&self, window: u32) -> Result<(), String> {
        self.conn.map_window(window).map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())?;
        // _NET_ACTIVE_WINDOW: source 2 (pager-like user request), timestamp 0.
        self.root_message(window, self.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0])
    }

    /// Ask the window to close as if its close button had been pressed.
    pub fn request_close(&self, window: u32) -> Result<(), String> {
        let event = ClientMessageEvent::new(
            32,
            window,
            self.atoms.WM_PROTOCOLS,
            [self.atoms.WM_DELETE_WINDOW, 0, 0, 0, 0],
        );
        self.conn
            .send_event(false, window, EventMask::NO_EVENT, event)
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }

    /// Outer frame rectangle [left, top, right, bottom] in root coordinates.
    fn frame(&self, window: u32) -> Result<[i32; 4], String> {
        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let origin = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let extents = self.cardinals(window, self.atoms._NET_FRAME_EXTENTS);
        let [left, right, top, bottom] = match extents.as_slice() {
            [l, r, t, b, ..] => [*l as i32, *r as i32, *t as i32, *b as i32],
            _ => [0; 4],
        };
        let x = i32::from(origin.dst_x) - left;
        let y = i32::from(origin.dst_y) - top;
        Ok([
            x,
            y,
            x + i32::from(geometry.width) + left + right,
            y + i32::from(geometry.height) + top + bottom,
        ])
    }

    /// Work area of the monitor nearest the window, excluding panels and docks.
    pub fn work_area(&self, window: u32) -> Result<[i32; 4], String> {
        let states = self.states(window);
        let a = &self.atoms;
        if states.contains(&a._NET_WM_STATE_HIDDEN)
            || (states.contains(&a._NET_WM_STATE_MAXIMIZED_VERT)
                && states.contains(&a._NET_WM_STATE_MAXIMIZED_HORZ))
        {
            return Err("Window is minimized or maximized".into());
        }
        let frame = self.frame(window)?;
        let monitor = self.monitor_for(frame)?;
        let desktop = self
            .cardinals(self.root, a._NET_CURRENT_DESKTOP)
            .first()
            .copied()
            .unwrap_or(0) as usize;
        let areas = self.cardinals(self.root, a._NET_WORKAREA);
        let work = match areas.get(desktop * 4..desktop * 4 + 4) {
            Some([x, y, w, h]) => {
                let (x, y) = (*x as i32, *y as i32);
                [x, y, x + *w as i32, y + *h as i32]
            }
            _ => monitor,
        };
        // _NET_WORKAREA spans every monitor; keep its insets on this one.
        let area = [
            monitor[0].max(work[0]),
            monitor[1].max(work[1]),
            monitor[2].min(work[2]),
            monitor[3].min(work[3]),
        ];
        if area[2] <= area[0] || area[3] <= area[1] {
            Ok(monitor)
        } else {
            Ok(area)
        }
    }

    fn monitor_for(&self, frame: [i32; 4]) -> Result<[i32; 4], String> {
        use x11rb::protocol::randr::ConnectionExt as _;
        let monitors = self
            .conn
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| {
                reply
                    .monitors
                    .iter()
                    .map(|m| {
                        let (x, y) = (i32::from(m.x), i32::from(m.y));
                        [x, y, x + i32::from(m.width), y + i32::from(m.height)]
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let center = ((frame[0] + frame[2]) / 2, (frame[1] + frame[3]) / 2);
        let distance = |m: &[i32; 4]| {
            let dx = (m[0] - center.0).max(center.0 - m[2]).max(0);
            let dy = (m[1] - center.1).max(center.1 - m[3]).max(0);
            i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
        };
        if let Some(monitor) = monitors.iter().min_by_key(|m| distance(m)) {
            return Ok(*monitor);
        }
        let screen = &self.conn.setup().roots[0];
        Ok([
            0,
            0,
            i32::from(screen.width_in_pixels),
            i32::from(screen.height_in_pixels),
        ])
    }

    /// Move and shrink the window so its frame fits the work area. Never raises it.
    pub fn fit(&self, window: u32, work: [i32; 4]) -> Result<(), String> {
        let states = self.states(window);
        if states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_VERT)
            && states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_HORZ)
        {
            return Ok(());
        }
        let [left, top, right, bottom] = work;
        if right <= left || bottom <= top {
            return Err("Monitor work area is empty".into());
        }
        let frame = self.frame(window)?;
        let (frame_w, frame_h) = (frame[2] - frame[0], frame[3] - frame[1]);
        let width = frame_w.min(right - left);
        let height = frame_h.min(bottom - top);
        let x = frame[0].clamp(left, right - width);
        let y = frame[1].clamp(top, bottom - height);
        if width == frame_w && height == frame_h && x == frame[0] && y == frame[1] {
            return Ok(());
        }
        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let client_w = (i32::from(geometry.width) - (frame_w - width)).max(1);
        let client_h = (i32::from(geometry.height) - (frame_h - height)).max(1);
        // Flags: NorthWest gravity (1), x|y|width|height present (bits 8-11),
        // source 2 (user-equivalent, so window managers honour it).
        let flags = 1 | (0xf << 8) | (2 << 12);
        self.root_message(
            window,
            self.atoms._NET_MOVERESIZE_WINDOW,
            [flags, x as u32, y as u32, client_w as u32, client_h as u32],
        )
    }
}
