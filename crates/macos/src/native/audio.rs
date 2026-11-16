//! CoreAudio listeners retain their block until synchronous removal completes.
use block2::RcBlock;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};
#[repr(C)]
#[derive(Clone, Copy)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}
// The CoreAudio callback ABI passes a pointer to this three-UInt32 C struct.
unsafe impl objc2::encode::Encode for Address {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
        "AudioObjectPropertyAddress",
        &[
            objc2::encode::Encoding::UInt,
            objc2::encode::Encoding::UInt,
            objc2::encode::Encoding::UInt,
        ],
    );
}
unsafe impl objc2::encode::RefEncode for Address {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&<Self as objc2::encode::Encode>::ENCODING);
}
const DEFAULT: Address = Address {
    selector: objc2_core_audio::kAudioHardwarePropertyDefaultOutputDevice,
    scope: u32::from_be_bytes(*b"glob"),
    element: 0,
};
const ALIVE: Address = Address {
    selector: objc2_core_audio::kAudioDevicePropertyDeviceIsAlive,
    ..DEFAULT
};
type Listener = block2::Block<dyn Fn(u32, *const Address)>;
#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectAddPropertyListenerBlock(
        object: u32,
        address: *const Address,
        queue: *mut std::ffi::c_void,
        block: &Listener,
    ) -> i32;
    fn AudioObjectRemovePropertyListenerBlock(
        object: u32,
        address: *const Address,
        queue: *mut std::ffi::c_void,
        block: &Listener,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const std::ffi::c_void,
        size: *mut u32,
        data: *mut std::ffi::c_void,
    ) -> i32;
}
fn read(object: u32, address: &Address) -> u32 {
    let mut value = 0u32;
    let mut size = 4;
    unsafe {
        if AudioObjectGetPropertyData(
            object,
            address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut u32).cast(),
        ) != 0
        {
            return 0;
        }
    }
    value
}
struct State {
    stopped: bool,
    device: u32,
    pending: Arc<AtomicU8>,
    alive: RcBlock<dyn Fn(u32, *const Address)>,
}
// CoreAudio serializes access through this mutex; blocks capture no AppKit objects.
unsafe impl Send for State {}
pub struct OutputWatcher {
    state: Arc<Mutex<State>>,
    changed: RcBlock<dyn Fn(u32, *const Address)>,
}
impl OutputWatcher {
    pub fn new() -> Result<Self, String> {
        let pending = Arc::new(AtomicU8::new(0));
        let p = pending.clone();
        let alive = RcBlock::new(move |_: u32, _: *const Address| {
            let device = read(1, &DEFAULT);
            p.store(
                if device != 0 && read(device, &ALIVE) != 0 {
                    2
                } else {
                    1
                },
                Ordering::Release,
            );
        });
        let state = Arc::new(Mutex::new(State {
            stopped: false,
            device: 0,
            pending,
            alive,
        }));
        let weak = Arc::downgrade(&state);
        let changed = RcBlock::new(move |_: u32, _: *const Address| {
            if let Some(state) = weak.upgrade() {
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                if state.stopped {
                    return;
                }
                unsafe {
                    if state.device != 0 {
                        AudioObjectRemovePropertyListenerBlock(
                            state.device,
                            &ALIVE,
                            std::ptr::null_mut(),
                            &state.alive,
                        );
                    }
                    state.device = read(1, &DEFAULT);
                    if state.device != 0 {
                        AudioObjectAddPropertyListenerBlock(
                            state.device,
                            &ALIVE,
                            std::ptr::null_mut(),
                            &state.alive,
                        );
                    }
                }
                state.pending.store(
                    if state.device != 0 && read(state.device, &ALIVE) != 0 {
                        2
                    } else {
                        1
                    },
                    Ordering::Release,
                );
            }
        });
        // Hold the state lock across installation and initialization so an early
        // default-device callback cannot install a duplicate alive listener.
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            if AudioObjectAddPropertyListenerBlock(1, &DEFAULT, std::ptr::null_mut(), &changed) != 0
            {
                return Err("Default audio output monitoring is unavailable".into());
            }
        }
        {
            s.device = read(1, &DEFAULT);
            unsafe {
                if s.device != 0 {
                    AudioObjectAddPropertyListenerBlock(
                        s.device,
                        &ALIVE,
                        std::ptr::null_mut(),
                        &s.alive,
                    );
                }
            }
        }
        drop(s);
        Ok(Self { state, changed })
    }
    pub fn take_available(&self) -> Option<bool> {
        match self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending
            .swap(0, Ordering::AcqRel)
        {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }
}
impl Drop for OutputWatcher {
    fn drop(&mut self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).stopped = true;
        unsafe {
            AudioObjectRemovePropertyListenerBlock(
                1,
                &DEFAULT,
                std::ptr::null_mut(),
                &self.changed,
            );
        }
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            if s.device != 0 {
                AudioObjectRemovePropertyListenerBlock(
                    s.device,
                    &ALIVE,
                    std::ptr::null_mut(),
                    &s.alive,
                );
            }
        }
    }
}
