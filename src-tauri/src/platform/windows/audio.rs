//! Core Audio backend: default output volume/mute (`IAudioEndpointVolume`), device list and
//! switching (`IMMDeviceEnumerator`, undocumented `IPolicyConfig`), per-app sessions
//! (`IAudioSessionManager2` / `ISimpleAudioVolume`) and change notifications.
//!
//! All calls run on the `kotilo-audio` MTA worker; the watcher owns its own MTA thread that
//! holds every registered callback and unregisters them before it exits.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::mpsc::{self, Sender};

use windows::core::{implement, Interface, Ref, BOOL, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, PROPERTYKEY};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, AudioSessionDisconnectReason, AudioSessionState,
    AudioSessionStateExpired, EDataFlow, ERole, IAudioSessionControl, IAudioSessionControl2,
    IAudioSessionEvents, IAudioSessionEvents_Impl, IAudioSessionManager2,
    IAudioSessionNotification, IAudioSessionNotification_Impl, IMMDevice, IMMDeviceEnumerator,
    IMMEndpoint, IMMNotificationClient, IMMNotificationClient_Impl, ISimpleAudioVolume,
    MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, DEVICE_STATE_ACTIVE,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::com::{os, ComGuard, Worker};
use crate::platform::{
    AudioDevice, AudioSession, AudioState, ChangeFn, PlatformError, Result, Watch,
};

static WORKER: Worker = Worker::new("kotilo-audio");

/// Runs `f` on the audio COM worker.
pub fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    WORKER.run(f)
}

thread_local! {
    static ENUMERATOR: RefCell<Option<IMMDeviceEnumerator>> = const { RefCell::new(None) };
}

fn create_enumerator() -> Result<IMMDeviceEnumerator> {
    // SAFETY: COM is initialised (MTA) on every thread that calls this.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(os)
}

/// Cached per worker thread (the enumerator is cheap to keep and costly-ish to create).
fn enumerator() -> Result<IMMDeviceEnumerator> {
    ENUMERATOR.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(e) = cell.as_ref() {
            return Ok(e.clone());
        }
        let e = create_enumerator()?;
        *cell = Some(e.clone());
        Ok(e)
    })
}

/// Copies and frees a COM-allocated string.
fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: `p` is a valid NUL-terminated string allocated with CoTaskMemAlloc by the callee;
    // it is freed exactly once here.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

fn device_id(d: &IMMDevice) -> Result<String> {
    // SAFETY: plain COM getter on a live device.
    Ok(take_pwstr(unsafe { d.GetId() }.map_err(os)?))
}

fn device_name(d: &IMMDevice) -> String {
    // SAFETY: property store read; the PROPVARIANT is cleared after conversion.
    let name = unsafe {
        d.OpenPropertyStore(STGM_READ).and_then(|store| {
            let mut pv = store.GetValue(&PKEY_Device_FriendlyName)?;
            let s = PropVariantToStringAlloc(&pv).map(take_pwstr);
            let _ = PropVariantClear(&mut pv);
            s
        })
    };
    name.ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Unknown device".into())
}

fn default_device(e: &IMMDeviceEnumerator) -> Option<IMMDevice> {
    // SAFETY: COM getter; E_NOTFOUND (no output at all) maps to None.
    unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) }.ok()
}

fn endpoint_volume(d: &IMMDevice) -> Result<IAudioEndpointVolume> {
    // SAFETY: activating a documented interface on a live device.
    unsafe { d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.map_err(os)
}

fn default_endpoint_volume() -> Result<IAudioEndpointVolume> {
    let e = enumerator()?;
    let d = default_device(&e).ok_or_else(|| PlatformError::Os("no default output".into()))?;
    endpoint_volume(&d)
}

pub fn devices() -> Result<Vec<AudioDevice>> {
    let e = enumerator()?;
    let default_id = default_device(&e).and_then(|d| device_id(&d).ok());
    // SAFETY: enumeration of active render endpoints; indices are within GetCount.
    let list = unsafe { e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) }.map_err(os)?;
    let count = unsafe { list.GetCount() }.map_err(os)?;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let Ok(d) = (unsafe { list.Item(i) }) else {
            continue;
        };
        let Ok(id) = device_id(&d) else { continue };
        out.push(AudioDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            name: device_name(&d),
            id,
        });
    }
    Ok(out)
}

pub fn state() -> Result<AudioState> {
    let e = enumerator()?;
    let devices = devices()?;
    let Some(def) = default_device(&e) else {
        return Ok(AudioState {
            device: None,
            volume: 0.0,
            muted: false,
            devices,
        });
    };
    let device = match devices.iter().find(|d| d.is_default) {
        Some(d) => d.clone(),
        None => AudioDevice {
            id: device_id(&def)?,
            name: device_name(&def),
            is_default: true,
        },
    };
    let ev = endpoint_volume(&def)?;
    // SAFETY: COM getters on a live endpoint.
    let (volume, muted) = unsafe {
        (
            ev.GetMasterVolumeLevelScalar().map_err(os)?,
            ev.GetMute().map_err(os)?.as_bool(),
        )
    };
    Ok(AudioState {
        device: Some(device),
        volume: volume.clamp(0.0, 1.0),
        muted,
        devices,
    })
}

pub fn volume() -> Result<f32> {
    let ev = default_endpoint_volume()?;
    // SAFETY: COM getter on a live endpoint.
    Ok(unsafe { ev.GetMasterVolumeLevelScalar() }
        .map_err(os)?
        .clamp(0.0, 1.0))
}

pub fn set_volume(level: f32) -> Result<()> {
    let ev = default_endpoint_volume()?;
    // SAFETY: COM setter; a null event context is allowed.
    unsafe { ev.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null()) }.map_err(os)
}

pub fn set_muted(muted: bool) -> Result<()> {
    let ev = default_endpoint_volume()?;
    // SAFETY: COM setter; a null event context is allowed.
    unsafe { ev.SetMute(muted, std::ptr::null()) }.map_err(os)
}

// ── Default device switching (undocumented IPolicyConfig) ───────────────────

/// Isolated so the undocumented interface (and its COM naming) stays out of the rest.
#[allow(non_snake_case, dead_code)]
mod policy {
    use std::ffi::c_void;
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, BOOL, GUID, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    /// `IPolicyConfig` (Windows 7+; what the Sound control panel uses). Undocumented: only the
    /// vtable slot of `SetDefaultEndpoint` matters, the other methods are never called, so
    /// their parameter types are opaque placeholders.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(
            &self,
            device: PCWSTR,
            default: BOOL,
            format: *mut *mut c_void,
        ) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(
            &self,
            device: PCWSTR,
            endpoint: *mut c_void,
            mix: *mut c_void,
        ) -> HRESULT;
        fn GetProcessingPeriod(
            &self,
            device: PCWSTR,
            default: BOOL,
            default_period: *mut i64,
            min_period: *mut i64,
        ) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        fn SetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: BOOL) -> HRESULT;
    }

    /// `CPolicyConfigClient`.
    pub const CLSID_POLICY_CONFIG_CLIENT: GUID =
        GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// # Safety
    /// `device` must point to a NUL-terminated endpoint id that outlives the call.
    pub unsafe fn set_default_endpoint(p: &IPolicyConfig, device: PCWSTR, role: ERole) -> HRESULT {
        p.SetDefaultEndpoint(device, role)
    }
}

use policy::{IPolicyConfig, CLSID_POLICY_CONFIG_CLIENT};

pub fn set_default_device(id: &str) -> Result<()> {
    let e = enumerator()?;
    let wid = HSTRING::from(id);
    // Only accept an active render endpoint; never hand arbitrary strings to IPolicyConfig.
    // SAFETY: documented COM lookups on a live enumerator/device.
    unsafe {
        let d = e
            .GetDevice(&wid)
            .map_err(|_| PlatformError::Os("unknown audio device".into()))?;
        let active = d.GetState().map_err(os)? == DEVICE_STATE_ACTIVE;
        let render = d
            .cast::<IMMEndpoint>()
            .and_then(|ep| ep.GetDataFlow())
            .map(|f| f == eRender)
            .unwrap_or(false);
        if !active || !render {
            return Err(PlatformError::Os("not an active output device".into()));
        }
    }
    // SAFETY: CPolicyConfigClient is an in-proc COM class; if it or the interface is missing
    // (future Windows), creation fails and we report it instead of crashing.
    let policy: IPolicyConfig =
        unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }.map_err(
            |e| {
                PlatformError::Os(format!(
                    "default-device switching unavailable: {}",
                    e.message()
                ))
            },
        )?;
    // Same roles as Settings ▸ Sound "Set as default" (communications is left alone).
    for role in [eConsole, eMultimedia] {
        // SAFETY: `wid` outlives the call; the vtable slot matches IPolicyConfig on Win7–11.
        unsafe { policy::set_default_endpoint(&policy, PCWSTR(wid.as_ptr()), role) }
            .ok()
            .map_err(|e| {
                PlatformError::Os(format!("SetDefaultEndpoint failed: {}", e.message()))
            })?;
    }
    Ok(())
}

// ── Per-app sessions ────────────────────────────────────────────────────────

fn session_manager(d: &IMMDevice) -> Result<IAudioSessionManager2> {
    // SAFETY: activating a documented interface on a live device.
    unsafe { d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) }.map_err(os)
}

/// Live (non-expired) session controls on the default output.
fn session_controls() -> Result<Vec<IAudioSessionControl>> {
    let e = enumerator()?;
    let Some(d) = default_device(&e) else {
        return Ok(Vec::new());
    };
    let mgr = session_manager(&d)?;
    // SAFETY: enumeration with indices within GetCount.
    unsafe {
        let en = mgr.GetSessionEnumerator().map_err(os)?;
        let n = en.GetCount().map_err(os)?;
        Ok((0..n)
            .filter_map(|i| en.GetSession(i).ok())
            .filter(|c| c.GetState().is_ok_and(|s| s != AudioSessionStateExpired))
            .collect())
    }
}

fn session_id(c: &IAudioSessionControl) -> Option<String> {
    let c2 = c.cast::<IAudioSessionControl2>().ok()?;
    // SAFETY: COM getter; the string is freed by take_pwstr.
    unsafe { c2.GetSessionInstanceIdentifier() }
        .ok()
        .map(take_pwstr)
}

fn find_session(id: &str) -> Result<ISimpleAudioVolume> {
    session_controls()?
        .into_iter()
        .find(|c| session_id(c).as_deref() == Some(id))
        .ok_or_else(|| PlatformError::Os("audio session not found".into()))?
        .cast::<ISimpleAudioVolume>()
        .map_err(os)
}

pub fn sessions() -> Result<Vec<AudioSession>> {
    let mut out = Vec::new();
    for c in session_controls()? {
        let Some(id) = session_id(&c) else { continue };
        let Ok(vol) = c.cast::<ISimpleAudioVolume>() else {
            continue;
        };
        // SAFETY: COM getters on a live session.
        let (volume, muted) = unsafe {
            (
                vol.GetMasterVolume().unwrap_or(1.0),
                vol.GetMute().map(|b| b.as_bool()).unwrap_or(false),
            )
        };
        out.push(AudioSession {
            id,
            name: session_name(&c),
            volume: volume.clamp(0.0, 1.0),
            muted,
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

pub fn set_session_volume(id: &str, level: f32) -> Result<()> {
    let v = find_session(id)?;
    // SAFETY: COM setter; a null event context is allowed.
    unsafe { v.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null()) }.map_err(os)
}

pub fn set_session_muted(id: &str, muted: bool) -> Result<()> {
    let v = find_session(id)?;
    // SAFETY: COM setter; a null event context is allowed.
    unsafe { v.SetMute(muted, std::ptr::null()) }.map_err(os)
}

fn session_name(c: &IAudioSessionControl) -> String {
    let Ok(c2) = c.cast::<IAudioSessionControl2>() else {
        return "Unknown app".into();
    };
    // SAFETY: COM getters on a live session.
    unsafe {
        if c2.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK {
            return "System sounds".into();
        }
        // Apps rarely set a display name; '@…' is an unresolved resource string.
        let display = c.GetDisplayName().map(take_pwstr).unwrap_or_default();
        if !display.trim().is_empty() && !display.starts_with('@') {
            return display;
        }
        c2.GetProcessId()
            .ok()
            .and_then(process_display_name)
            .unwrap_or_else(|| "Unknown app".into())
    }
}

/// "Spotify" / "Microsoft Edge" from the exe's version resource, else the exe stem.
fn process_display_name(pid: u32) -> Option<String> {
    let path = process_path(pid)?;
    file_description(&path).or_else(|| {
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
    })
}

fn process_path(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning; the buffer length is passed in/out.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let r =
            QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn file_description(path: &str) -> Option<String> {
    let wpath = HSTRING::from(path);
    // SAFETY: the version block is sized by GetFileVersionInfoSizeW; VerQueryValueW returns
    // pointers into that block, which outlives every read below.
    unsafe {
        let size = GetFileVersionInfoSizeW(&wpath, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&wpath, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let (lang, cp) = if VerQueryValueW(
            data.as_ptr().cast(),
            windows::core::w!(r"\VarFileInfo\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && len >= 4
            && !ptr.is_null()
        {
            let p = ptr as *const u16;
            (p.read_unaligned(), p.add(1).read_unaligned())
        } else {
            (0x0409, 0x04b0)
        };
        let key = HSTRING::from(format!(
            r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription"
        ));
        if !VerQueryValueW(data.as_ptr().cast(), &key, &mut ptr, &mut len).as_bool()
            || ptr.is_null()
            || len == 0
        {
            return None;
        }
        let chars = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let s = String::from_utf16_lossy(chars);
        let s = s.trim_end_matches('\0').trim();
        (!s.is_empty()).then(|| s.to_string())
    }
}

// ── Change notifications ────────────────────────────────────────────────────

enum Msg {
    /// Default output changed: re-attach the endpoint and session callbacks.
    Rebind,
    /// Session created/expired/disconnected: re-attach per-session callbacks.
    Sessions,
    Stop,
}

#[implement(IMMNotificationClient)]
struct DeviceEvents {
    tx: Sender<Msg>,
    on_change: ChangeFn,
}

impl IMMNotificationClient_Impl for DeviceEvents_Impl {
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        // Fires once per role and flow; we only track the console render default.
        if flow == eRender && role == eConsole {
            let _ = self.tx.send(Msg::Rebind);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        // Very chatty on some drivers and nothing we show depends on it.
        Ok(())
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeEvents {
    on_change: ChangeFn,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeEvents_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
}

#[implement(IAudioSessionNotification)]
struct SessionCreated {
    tx: Sender<Msg>,
}

impl IAudioSessionNotification_Impl for SessionCreated_Impl {
    fn OnSessionCreated(&self, _session: Ref<IAudioSessionControl>) -> windows::core::Result<()> {
        // Registering callbacks from inside this callback can deadlock; defer to the watcher.
        let _ = self.tx.send(Msg::Sessions);
        Ok(())
    }
}

#[implement(IAudioSessionEvents)]
struct SessionEvents {
    tx: Sender<Msg>,
    on_change: ChangeFn,
}

impl IAudioSessionEvents_Impl for SessionEvents_Impl {
    fn OnDisplayNameChanged(&self, _n: &PCWSTR, _c: *const GUID) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
    fn OnIconPathChanged(&self, _p: &PCWSTR, _c: *const GUID) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnSimpleVolumeChanged(
        &self,
        _v: f32,
        _m: BOOL,
        _c: *const GUID,
    ) -> windows::core::Result<()> {
        (self.on_change)();
        Ok(())
    }
    fn OnChannelVolumeChanged(
        &self,
        _n: u32,
        _v: *const f32,
        _ch: u32,
        _c: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnGroupingParamChanged(
        &self,
        _g: *const GUID,
        _c: *const GUID,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnStateChanged(&self, state: AudioSessionState) -> windows::core::Result<()> {
        if state == AudioSessionStateExpired {
            let _ = self.tx.send(Msg::Sessions);
        }
        Ok(())
    }
    fn OnSessionDisconnected(&self, _r: AudioSessionDisconnectReason) -> windows::core::Result<()> {
        let _ = self.tx.send(Msg::Sessions);
        Ok(())
    }
}

/// Callbacks attached to the current default output. Dropping unregisters all of them.
struct Bound {
    endpoint: IAudioEndpointVolume,
    volume_cb: IAudioEndpointVolumeCallback,
    manager: Option<(IAudioSessionManager2, IAudioSessionNotification)>,
    sessions: Vec<(IAudioSessionControl, IAudioSessionEvents)>,
}

impl Bound {
    fn bind(e: &IMMDeviceEnumerator, tx: &Sender<Msg>, on_change: &ChangeFn) -> Option<Bound> {
        let d = default_device(e)?;
        let endpoint = endpoint_volume(&d).ok()?;
        let volume_cb: IAudioEndpointVolumeCallback = VolumeEvents {
            on_change: on_change.clone(),
        }
        .into();
        // SAFETY: the callback stays alive in `Bound` until it is unregistered in Drop.
        if let Err(err) = unsafe { endpoint.RegisterControlChangeNotify(&volume_cb) } {
            tracing::warn!("audio volume notifications unavailable: {}", err.message());
            return None;
        }
        let manager = session_manager(&d).ok().and_then(|mgr| {
            let cb: IAudioSessionNotification = SessionCreated { tx: tx.clone() }.into();
            // SAFETY: as above; unregistered in Drop.
            unsafe { mgr.RegisterSessionNotification(&cb) }
                .ok()
                .map(|_| (mgr, cb))
        });
        let mut b = Bound {
            endpoint,
            volume_cb,
            manager,
            sessions: Vec::new(),
        };
        b.bind_sessions(tx, on_change);
        Some(b)
    }

    /// (Re)attaches per-session callbacks. Enumerating is also what arms
    /// `OnSessionCreated` (documented Core Audio quirk).
    fn bind_sessions(&mut self, tx: &Sender<Msg>, on_change: &ChangeFn) {
        self.unbind_sessions();
        let Some((mgr, _)) = self.manager.as_ref() else {
            return;
        };
        // SAFETY: enumeration within GetCount; callbacks kept alive in `self.sessions`.
        unsafe {
            let Ok(en) = mgr.GetSessionEnumerator() else {
                return;
            };
            let n = en.GetCount().unwrap_or(0);
            for i in 0..n {
                let Ok(c) = en.GetSession(i) else { continue };
                if c.GetState().is_ok_and(|s| s == AudioSessionStateExpired) {
                    continue;
                }
                let cb: IAudioSessionEvents = SessionEvents {
                    tx: tx.clone(),
                    on_change: on_change.clone(),
                }
                .into();
                if c.RegisterAudioSessionNotification(&cb).is_ok() {
                    self.sessions.push((c, cb));
                }
            }
        }
    }

    fn unbind_sessions(&mut self) {
        for (c, cb) in self.sessions.drain(..) {
            // SAFETY: unregistering a callback registered on this same control.
            let _ = unsafe { c.UnregisterAudioSessionNotification(&cb) };
        }
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        self.unbind_sessions();
        // SAFETY: unregistering callbacks registered in `bind`.
        unsafe {
            if let Some((mgr, cb)) = self.manager.take() {
                let _ = mgr.UnregisterSessionNotification(&cb);
            }
            let _ = self.endpoint.UnregisterControlChangeNotify(&self.volume_cb);
        }
    }
}

struct StopOnDrop(Sender<Msg>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(Msg::Stop);
    }
}

pub fn watch(on_change: ChangeFn) -> Result<Watch> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);
    let thread_tx = tx.clone();
    std::thread::Builder::new()
        .name("kotilo-audio-watch".into())
        .spawn(move || {
            let _com = match ComGuard::init() {
                Ok(g) => g,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let setup = create_enumerator().and_then(|e| {
                let client: IMMNotificationClient = DeviceEvents {
                    tx: thread_tx.clone(),
                    on_change: on_change.clone(),
                }
                .into();
                // SAFETY: the client stays alive on this thread until it is unregistered below.
                unsafe { e.RegisterEndpointNotificationCallback(&client) }.map_err(os)?;
                Ok((e, client))
            });
            let (e, client) = match setup {
                Ok(v) => {
                    let _ = ready_tx.send(Ok(()));
                    v
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            tracing::debug!("audio watcher started");
            let mut bound = Bound::bind(&e, &thread_tx, &on_change);
            while let Ok(msg) = rx.recv() {
                match msg {
                    Msg::Stop => break,
                    Msg::Rebind => {
                        drop(bound.take());
                        bound = Bound::bind(&e, &thread_tx, &on_change);
                        on_change();
                    }
                    Msg::Sessions => {
                        if let Some(b) = bound.as_mut() {
                            b.bind_sessions(&thread_tx, &on_change);
                        }
                        on_change();
                    }
                }
            }
            drop(bound);
            // SAFETY: unregisters the client registered above, on the same thread.
            let _ = unsafe { e.UnregisterEndpointNotificationCallback(&client) };
            tracing::debug!("audio watcher stopped");
        })
        .map_err(|e| PlatformError::Os(format!("cannot start audio watcher: {e}")))?;
    ready_rx
        .recv()
        .map_err(|_| PlatformError::Os("audio watcher exited during start".into()))??;
    Ok(Watch::new(StopOnDrop(tx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    #[ignore = "reads real audio devices (read-only)"]
    fn real_audio_state_and_sessions() {
        let s = run(state).expect("audio state");
        println!(
            "default: {:?}; volume {:.2}; muted {}; {} device(s)",
            s.device.as_ref().map(|d| &d.name),
            s.volume,
            s.muted,
            s.devices.len()
        );
        for d in &s.devices {
            println!("  device {:?} default={}", d.name, d.is_default);
        }
        assert!((0.0..=1.0).contains(&s.volume));
        assert_eq!(
            s.devices.iter().filter(|d| d.is_default).count(),
            usize::from(s.device.is_some())
        );
        let sessions = run(sessions).expect("sessions");
        for x in &sessions {
            println!(
                "  session {:?} vol {:.2} muted {}",
                x.name, x.volume, x.muted
            );
            assert!((0.0..=1.0).contains(&x.volume));
        }
    }

    #[test]
    #[ignore = "registers real Core Audio callbacks (read-only)"]
    fn real_audio_watch_starts_and_stops() {
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let w = watch(Arc::new(move || {
            h.fetch_add(1, Ordering::Relaxed);
        }))
        .expect("watch");
        std::thread::sleep(std::time::Duration::from_millis(300));
        drop(w);
        std::thread::sleep(std::time::Duration::from_millis(100));
        println!(
            "audio change callbacks during test: {}",
            hits.load(Ordering::Relaxed)
        );
    }

    #[test]
    #[ignore = "creates the undocumented IPolicyConfig object without calling it"]
    fn real_policy_config_is_available() {
        run(|| {
            // SAFETY: creation only; no method is invoked, the default device is untouched.
            let p: windows::core::Result<IPolicyConfig> =
                unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) };
            Ok(p.is_ok())
        })
        .map(|ok| assert!(ok, "IPolicyConfig unavailable"))
        .expect("worker");
    }

    #[test]
    #[ignore = "reads real audio devices (read-only)"]
    fn real_set_default_rejects_unknown_id() {
        assert!(run(|| set_default_device("not-a-device")).is_err());
    }
}
