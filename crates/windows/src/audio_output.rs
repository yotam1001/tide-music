//! Coalesced default-output notifications. No device identifiers leave callbacks.
use super::native::Apartment;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use windows::core::{implement, PCWSTR};
use windows::Win32::{
    Foundation::PROPERTYKEY,
    Media::Audio::{
        eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
    },
    System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
};

#[implement(IMMNotificationClient)]
struct Notifications {
    pending: Arc<AtomicU8>,
}

#[allow(non_snake_case)]
impl IMMNotificationClient_Impl for Notifications_Impl {
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        device: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eRender && role == eMultimedia {
            self.pending
                .store(if device.is_null() { 1 } else { 2 }, Ordering::Release);
        }
        Ok(())
    }
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Own and drop on the creating UI thread, independently of SMTC being enabled.
pub struct OutputWatcher {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    pending: Arc<AtomicU8>,
    _apartment: Apartment,
}
impl OutputWatcher {
    pub fn new() -> Result<Self, String> {
        let apartment = Apartment::initialize()?;
        // SAFETY: initialized apartment, system COM class and typed interface.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
                .map_err(|_| "Default audio output monitoring is unavailable.".to_string())?;
        let pending = Arc::new(AtomicU8::new(0));
        let client: IMMNotificationClient = Notifications {
            pending: pending.clone(),
        }
        .into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
            .map_err(|_| "Default audio output monitoring is unavailable.".to_string())?;
        Ok(Self {
            enumerator,
            client,
            pending,
            _apartment: apartment,
        })
    }
    pub fn take_available(&self) -> Option<bool> {
        match self.pending.swap(0, Ordering::AcqRel) {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        }
    }
}
impl Drop for OutputWatcher {
    fn drop(&mut self) {
        // Unsubscribe before releasing interfaces/apartment. A late callback can
        // touch only its own Arc, never the UI, playback state or freed memory.
        unsafe {
            let _ = self
                .enumerator
                .UnregisterEndpointNotificationCallback(&self.client);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        core::w,
        Win32::Media::Audio::{eCapture, eCommunications, eConsole},
    };
    #[test]
    fn notifications_ignore_microphones_and_calls_and_coalesce_output_changes() {
        let pending = Arc::new(AtomicU8::new(0));
        let client: IMMNotificationClient = Notifications {
            pending: pending.clone(),
        }
        .into();
        unsafe {
            client
                .OnDefaultDeviceChanged(eCapture, eMultimedia, w!("fixture"))
                .unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eCommunications, w!("fixture"))
                .unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eConsole, w!("fixture"))
                .unwrap();
            assert_eq!(pending.load(Ordering::Acquire), 0);
            client
                .OnDefaultDeviceChanged(eRender, eMultimedia, PCWSTR::null())
                .unwrap();
            assert_eq!(pending.load(Ordering::Acquire), 1);
            client
                .OnDefaultDeviceChanged(eRender, eMultimedia, w!("fixture"))
                .unwrap();
            assert_eq!(pending.swap(0, Ordering::AcqRel), 2);
            assert_eq!(pending.load(Ordering::Acquire), 0);
        }
    }
}
