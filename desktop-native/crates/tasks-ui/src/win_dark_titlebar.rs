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
//!
//! Implementation note: we declare the Win32 types + entry points
//! we need with bare `extern "system"` blocks rather than pulling
//! in `windows-sys`. The four functions + four constants are
//! tiny, the surface is stable across decades of Win32 history,
//! and skipping `windows-sys` removes a transitive dep + insulates
//! us from breaking changes between its 0.59 / 0.60 / 0.61 shapes.

use std::ffi::c_void;
use std::time::Duration;

// ----- minimal Win32 surface --------------------------------------------

// Win32 typedefs we need. `HWND` is an opaque pointer type; using
// `*mut c_void` matches what windows-sys uses under the hood.
#[allow(non_camel_case_types)]
type HWND = *mut c_void;
#[allow(non_camel_case_types)]
type BOOL = i32;
#[allow(non_camel_case_types)]
type LPARAM = isize;
#[allow(non_camel_case_types)]
type DWORD = u32;
#[allow(non_camel_case_types)]
type HRESULT = i32;

const TRUE: BOOL = 1;

/// `DWMWINDOWATTRIBUTE::DWMWA_USE_IMMERSIVE_DARK_MODE` — the
/// numeric value is fixed by the DWM API and shipped in Windows
/// 10 1903 onward.
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;

/// `WNDENUMPROC` — the callback type EnumWindows takes.
type WndEnumProc = unsafe extern "system" fn(hwnd: HWND, lparam: LPARAM) -> BOOL;

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: WndEnumProc, lparam: LPARAM) -> BOOL;
    fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut DWORD) -> DWORD;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> DWORD;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        attribute: u32,
        value: *const c_void,
        size: u32,
    ) -> HRESULT;
}

// ----- polling thread ----------------------------------------------------

/// Polling interval — fast enough that a freshly-opened Settings
/// window isn't visibly white for more than ~half a second, slow
/// enough that the EnumWindows loop is invisible in profilers.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Spawn the polling thread. Detached; exits when the process
/// does. Safe to call once at startup.
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

    let our_pid = unsafe { GetCurrentProcessId() };

    loop {
        apply_to_all_top_level(our_pid);
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn apply_to_all_top_level(our_pid: DWORD) {
    // Pass our pid through `lparam`; the callback compares each
    // window's owner-process to it before applying.
    let lparam: LPARAM = our_pid as LPARAM;
    unsafe {
        EnumWindows(enum_proc, lparam);
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let our_pid = lparam as DWORD;
    let mut win_pid: DWORD = 0;
    GetWindowThreadProcessId(hwnd, &mut win_pid);
    if win_pid == our_pid {
        apply_dark(hwnd);
    }
    TRUE
}

fn apply_dark(hwnd: HWND) {
    let enabled: u32 = 1;
    let size = std::mem::size_of::<u32>() as u32;
    unsafe {
        // Return value ignored: 0 (S_OK) means success; anything
        // else is "OS too old" or "this attribute isn't supported
        // on this Windows version" — both cases we tolerate
        // silently.
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const u32 as *const c_void,
            size,
        );
    }
}
