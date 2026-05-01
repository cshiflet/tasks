//! Force-apply the Windows 11 immersive-dark-mode attribute on
//! every top-level window of our own process, regardless of
//! whether `QT_QPA_PLATFORM=windows:darkmode=2` propagated
//! correctly. Qt 6.6.3's QQuickWindow sometimes drops the
//! attribute depending on init ordering, leaving users with a
//! white native title bar on a dark-mode-configured Windows.
//!
//! Approach: a daemon thread that polls every ~500 ms,
//! enumerates HWNDs owned by our process, and calls
//! `DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE,
//! &1u32, 4)` on each. Idempotent; cheap (few syscalls per
//! second). New windows opened during runtime (Settings, edit
//! dialog) get the attribute on the next poll cycle.
//!
//! Module is `cfg(target_os = "windows")`-gated at the call site
//! in `main.rs`, so non-Windows builds never compile this code.

use std::time::Duration;

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

/// Polling interval — fast enough that a freshly-opened Settings
/// window isn't visibly white for more than ~half a second, slow
/// enough that the EnumWindows loop is invisible in profilers.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Spawn the polling thread. Detached; exits when the process
/// does. Safe to call once at startup; calling more than once is a
/// programming error and produces extra (harmless) syscall load.
pub fn start() {
    std::thread::Builder::new()
        .name("win-dark-titlebar".into())
        .spawn(poll_loop)
        .ok();
}

fn poll_loop() {
    // Sleep once before the first cycle so the engine has time to
    // create the main window. Without this we'd race the first
    // EnumWindows pass against window creation and find nothing.
    std::thread::sleep(POLL_INTERVAL);

    let our_pid = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };

    loop {
        apply_to_all_top_level(our_pid);
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn apply_to_all_top_level(our_pid: u32) {
    // EnumWindows takes a `WNDENUMPROC` (extern "system") + an
    // `LPARAM` we use to pass the pid through to the callback,
    // so no static state is needed despite the signature looking
    // awkward. LPARAM is a plain isize alias under windows-sys.
    let lparam: LPARAM = our_pid as isize;
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows(Some(enum_proc), lparam);
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let our_pid = lparam as u32;
    let mut win_pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut win_pid);
    if win_pid != our_pid {
        return TRUE;
    }
    apply_dark(hwnd);
    TRUE
}

fn apply_dark(hwnd: HWND) {
    let enabled: u32 = 1;
    let size = std::mem::size_of::<u32>() as u32;
    unsafe {
        // Return value ignored: a 0 means success; anything else
        // is "OS too old" or "this attribute isn't supported on
        // this Windows version" — both cases we tolerate silently.
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const u32 as *const _,
            size,
        );
    }
}
