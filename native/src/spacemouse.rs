use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub invert: [bool; 6],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            invert: [false, true, false, false, false, false],
        }
    }
}
impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|p| {
            std::path::PathBuf::from(p)
                .join("StepFast")
                .join("navigation-v2.bin")
        })
    }
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| bincode::deserialize(&b).ok())
            .or_else(|| {
                let path = Self::path()?.with_file_name("navigation-v1.bin");
                let bytes = std::fs::read(path).ok()?;
                let mut old: Self = bincode::deserialize(&bytes).ok()?;
                old.invert[1] = true;
                Some(old)
            })
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(path) = Self::path() {
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(
                path,
                bincode::serialize(self).map_err(std::io::Error::other)?,
            )?;
        }
        Ok(())
    }
    pub fn apply(&self, mut axes: [f32; 6]) -> [f32; 6] {
        for (axis, invert) in axes.iter_mut().zip(self.invert) {
            if invert {
                *axis = -*axis;
            }
        }
        axes
    }
}
pub struct State {
    pub axes: [f32; 6],
    pub last: Instant,
    pub seen: bool,
    pub fit: bool,
    pub buttons: u8,
    pub error: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            axes: [0.0; 6],
            last: Instant::now() - Duration::from_secs(1),
            seen: false,
            fit: false,
            buttons: 0,
            error: None,
        }
    }
}
// HID report 1: translation, 2: rotation, 3: buttons. Receivers can combine axes.
fn report(bytes: &[u8], s: &mut State) {
    let axis = |i: usize| {
        let v = i16::from_le_bytes([bytes[i], bytes[i + 1]]) as f32;
        if v.abs() < 12.0 {
            0.0
        } else {
            (v / 350.0).clamp(-2.0, 2.0)
        }
    };
    match bytes.first() {
        Some(1) if bytes.len() >= 7 => {
            s.axes[..3].copy_from_slice(&[axis(1), axis(3), axis(5)]);
            if bytes.len() >= 13 {
                s.axes[3..].copy_from_slice(&[axis(7), axis(9), axis(11)]);
            }
        }
        Some(2) if bytes.len() >= 7 => s.axes[3..].copy_from_slice(&[axis(1), axis(3), axis(5)]),
        Some(3) if bytes.len() >= 2 => {
            // Driver Base.xml: report 03, mask 0002 = V3DK_FIT.
            if bytes[1] & 2 != 0 && s.buttons & 2 == 0 {
                s.fit = true;
            }
            s.buttons = bytes[1];
        }
        _ => return,
    }
    s.seen = true;
    s.last = Instant::now();
}
pub fn start(ctx: eframe::egui::Context) -> Arc<Mutex<State>> {
    let state = Arc::new(Mutex::new(State::default()));
    #[cfg(windows)]
    {
        let shared = state.clone();
        std::thread::spawn(move || unsafe {
            win::run(shared, ctx);
        });
    }
    state
}
#[cfg(windows)]
mod win {
    use super::*;
    use std::{mem::size_of, ptr::null_mut, sync::OnceLock};
    use windows_sys::Win32::{
        Foundation::*,
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
        UI::{Input::*, WindowsAndMessaging::*},
    };
    static SHARED: OnceLock<(Arc<Mutex<State>>, eframe::egui::Context)> = OnceLock::new();
    pub unsafe fn run(state: Arc<Mutex<State>>, ctx: eframe::egui::Context) {
        let _ = SHARED.set((state.clone(), ctx));
        let name: Vec<u16> = "StepFast.RawInput\0".encode_utf16().collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: name.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            0,
            name.as_ptr(),
            name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            std::ptr::null(),
        );
        let device = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 8,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        if hwnd.is_null()
            || RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) == 0
        {
            state.lock().unwrap().error =
                Some(format!("Raw Input kunde inte starta ({})", GetLastError()));
            return;
        }
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    unsafe extern "system" fn proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_INPUT {
            // Message-only window receives INPUTSINK; gate navigation on app focus.
            let mut pid = 0;
            GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
            if pid == GetCurrentProcessId() {
                read(lparam as HRAWINPUT);
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
    unsafe fn read(handle: HRAWINPUT) {
        let mut bytes = 0;
        let header_size = size_of::<RAWINPUTHEADER>() as u32;
        if GetRawInputData(handle, RID_INPUT, null_mut(), &mut bytes, header_size) == u32::MAX
            || bytes < header_size + 8
            || bytes > 65536
        {
            return;
        }
        let mut buffer =
            vec![0usize; (bytes as usize + size_of::<usize>() - 1) / size_of::<usize>()];
        let read = GetRawInputData(
            handle,
            RID_INPUT,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
            header_size,
        );
        if read == u32::MAX || read < header_size + 8 {
            return;
        }
        let raw = &*(buffer.as_ptr() as *const RAWINPUTHEADER);
        if raw.dwType != RIM_TYPEHID {
            return;
        }
        let mut info = RID_DEVICE_INFO {
            cbSize: size_of::<RID_DEVICE_INFO>() as u32,
            ..std::mem::zeroed()
        };
        let mut size = info.cbSize;
        if GetRawInputDeviceInfoW(
            raw.hDevice,
            RIDI_DEVICEINFO,
            (&mut info as *mut RID_DEVICE_INFO).cast(),
            &mut size,
        ) == u32::MAX
        {
            return;
        }
        let vendor = info.Anonymous.hid.dwVendorId;
        if vendor != 0x256f && vendor != 0x046d {
            return;
        }
        let data = std::slice::from_raw_parts(buffer.as_ptr() as *const u8, read as usize);
        let offset = header_size as usize;
        let packet_size = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if packet_size == 0 || count > data.len().saturating_sub(offset + 8) / packet_size {
            return;
        }
        if let Some((state, ctx)) = SHARED.get() {
            if let Ok(mut s) = state.lock() {
                for packet in
                    data[offset + 8..offset + 8 + packet_size * count].chunks_exact(packet_size)
                {
                    report(packet, &mut s);
                }
            }
            ctx.request_repaint();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fit_button_uses_second_bit_and_rising_edge() {
        let mut s = State::default();
        report(&[3, 1], &mut s);
        assert!(!s.fit);
        report(&[3, 2], &mut s);
        assert!(s.fit);
        s.fit = false;
        report(&[3, 2], &mut s);
        assert!(!s.fit);
        report(&[3, 0], &mut s);
        report(&[3, 2], &mut s);
        assert!(s.fit);
    }
    #[test]
    fn translation_rotation_combined_and_truncated() {
        let mut s = State::default();
        report(&[1, 94, 1, 162, 254, 0, 0, 0, 0, 175, 0, 0, 0], &mut s);
        assert_eq!(s.axes, [1.0, -1.0, 0.0, 0.0, 0.5, 0.0]);
        assert!(s.seen);
        report(&[2, 0, 0, 0, 0, 94, 1], &mut s);
        assert_eq!(s.axes[5], 1.0);
        let previous = s.axes;
        report(&[1, 0], &mut s);
        assert_eq!(s.axes, previous);
        report(&[1, 0, 0, 0, 0, 0, 0], &mut s);
        assert_eq!(&s.axes[..3], &[0.0; 3]);
    }
}
