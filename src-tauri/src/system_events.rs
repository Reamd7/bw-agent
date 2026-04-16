//! System event detection for vault locking.
//!
//! Detects idle time, sleep/suspend, screen lock, and shutdown/restart across
//! Windows and macOS. Locks the vault by clearing keys and emitting a
//! `lock-state-changed` event when the configured system event fires.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc, OnceLock,
};
use std::time::Duration;

// ─── Shared state ────────────────────────────────────────────────────────────

const IDLE_DISABLED: u64 = 0;

const LOCK_MODE_NEVER: u8 = 0;
const LOCK_MODE_TIMEOUT: u8 = 1;
const LOCK_MODE_SYSTEM_IDLE: u8 = 2;
const LOCK_MODE_ON_SLEEP: u8 = 3;
const LOCK_MODE_ON_LOCK: u8 = 4;
const LOCK_MODE_ON_RESTART: u8 = 5;

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
static AGENT_STATE: OnceLock<Arc<tokio::sync::Mutex<bw_agent::state::State>>> = OnceLock::new();
static LOCK_MODE_KIND: AtomicU8 = AtomicU8::new(LOCK_MODE_NEVER);
static IDLE_THRESHOLD_SECONDS: AtomicU64 = AtomicU64::new(IDLE_DISABLED);
static IDLE_TRIGGERED: AtomicBool = AtomicBool::new(false);
static IDLE_THREAD_STARTED: AtomicBool = AtomicBool::new(false);

// ─── Public API ──────────────────────────────────────────────────────────────

/// Initialize platform-specific system event listeners.
///
/// On Windows: subclasses the main HWND for WM_WTSSESSION_CHANGE,
/// WM_POWERBROADCAST, WM_QUERYENDSESSION.
///
/// On macOS: subscribes to NSWorkspace sleep/wake notifications and
/// `com.apple.screenIsLocked` via NSDistributedNotificationCenter.
///
/// On both: starts an idle-polling thread if the lock mode is `SystemIdle`.
pub fn init(
    app_handle: &tauri::AppHandle,
    lock_mode: &bw_agent::config::LockMode,
    agent_state: Arc<tokio::sync::Mutex<bw_agent::state::State>>,
) -> Result<(), String> {
    let _ = APP_HANDLE.set(app_handle.clone());
    let _ = AGENT_STATE.set(agent_state);

    set_lock_mode(lock_mode);

    #[cfg(target_os = "windows")]
    platform_windows::init_platform(app_handle)?;

    #[cfg(target_os = "macos")]
    platform_macos::init_platform(app_handle)?;

    if let bw_agent::config::LockMode::SystemIdle { .. } = lock_mode {
        ensure_idle_thread_started()?;
    }

    log::info!("System event listeners initialized (lock_mode={lock_mode:?})");
    Ok(())
}

/// Update idle threshold from the idle-polling thread. Pass `None` to disable.
pub fn update_idle_threshold(seconds: Option<u64>) {
    IDLE_THRESHOLD_SECONDS.store(seconds.unwrap_or(IDLE_DISABLED), Ordering::SeqCst);
    if seconds.is_none() {
        IDLE_TRIGGERED.store(false, Ordering::SeqCst);
    }
}

/// Hot-reload lock mode at runtime. Updates the active mode and idle threshold.
pub fn set_lock_mode(lock_mode: &bw_agent::config::LockMode) {
    LOCK_MODE_KIND.store(lock_mode_kind(lock_mode), Ordering::SeqCst);

    match lock_mode {
        bw_agent::config::LockMode::SystemIdle { seconds } => {
            update_idle_threshold(Some(*seconds));
            if let Err(error) = ensure_idle_thread_started() {
                log::error!("failed to start idle polling thread: {error}");
            }
        }
        _ => update_idle_threshold(None),
    }
}

// ─── Shared internals ────────────────────────────────────────────────────────

/// Lock the vault from any thread (spawns on Tauri's async runtime).
fn lock_vault(reason: &str) {
    let (Some(app_handle), Some(agent_state)) = (APP_HANDLE.get(), AGENT_STATE.get()) else {
        log::warn!("system lock event fired before initialization: {reason}");
        return;
    };

    let app_handle = app_handle.clone();
    let agent_state = Arc::clone(agent_state);
    let reason = reason.to_string();

    tauri::async_runtime::spawn(async move {
        let mut state = agent_state.lock().await;
        let was_unlocked = state.is_unlocked();
        state.clear();
        drop(state);

        log::info!("Vault locked due to system event: {reason} (was_unlocked={was_unlocked})");

        if let Err(error) = crate::events::emit_lock_state_changed(&app_handle, true) {
            log::error!("failed to emit lock-state-changed after {reason}: {error}");
        }
    });
}

fn ensure_idle_thread_started() -> Result<(), String> {
    if IDLE_THREAD_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let spawn_result = std::thread::Builder::new()
        .name("system-idle-poll".to_string())
        .spawn(idle_poll_loop);

    if let Err(error) = spawn_result {
        IDLE_THREAD_STARTED.store(false, Ordering::SeqCst);
        return Err(format!("failed to spawn idle polling thread: {error}"));
    }

    Ok(())
}

fn idle_poll_loop() {
    loop {
        let threshold = IDLE_THRESHOLD_SECONDS.load(Ordering::Relaxed);

        if threshold == IDLE_DISABLED {
            IDLE_TRIGGERED.store(false, Ordering::Relaxed);
        } else if let Some(idle_seconds) = current_idle_seconds() {
            if idle_seconds >= threshold {
                if !IDLE_TRIGGERED.swap(true, Ordering::SeqCst) {
                    lock_vault("idle");
                }
            } else {
                IDLE_TRIGGERED.store(false, Ordering::SeqCst);
            }
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}

fn lock_mode_kind(lock_mode: &bw_agent::config::LockMode) -> u8 {
    match lock_mode {
        bw_agent::config::LockMode::Never => LOCK_MODE_NEVER,
        bw_agent::config::LockMode::Timeout { .. } => LOCK_MODE_TIMEOUT,
        bw_agent::config::LockMode::SystemIdle { .. } => LOCK_MODE_SYSTEM_IDLE,
        bw_agent::config::LockMode::OnSleep => LOCK_MODE_ON_SLEEP,
        bw_agent::config::LockMode::OnLock => LOCK_MODE_ON_LOCK,
        bw_agent::config::LockMode::OnRestart => LOCK_MODE_ON_RESTART,
    }
}

// ─── Idle time (platform-specific) ──────────────────────────────────────────

#[cfg(target_os = "windows")]
fn current_idle_seconds() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii) == 0 {
            log::warn!("GetLastInputInfo failed: {}", std::io::Error::last_os_error());
            return None;
        }
        let elapsed_ms = GetTickCount().wrapping_sub(lii.dwTime);
        Some((elapsed_ms / 1000) as u64)
    }
}

#[cfg(target_os = "macos")]
fn current_idle_seconds() -> Option<u64> {
    // CGEventSourceSecondsSinceLastEventType is not wrapped by core-graphics crate,
    // so we declare the FFI binding directly. Queries the HID system for how long
    // ago the last input event occurred — no keystroke interception.
    use core_graphics::event_source::CGEventSourceStateID;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(
            state_id: CGEventSourceStateID,
            event_type: u32,
        ) -> f64;
    }

    // kCGAnyInputEventType = ~0 (all input event types)
    const K_CG_ANY_INPUT_EVENT_TYPE: u32 = !0u32;

    let idle_time = unsafe {
        CGEventSourceSecondsSinceLastEventType(
            CGEventSourceStateID::HIDSystemState,
            K_CG_ANY_INPUT_EVENT_TYPE,
        )
    };

    if idle_time >= 0.0 {
        Some(idle_time as u64)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn current_idle_seconds() -> Option<u64> {
    None // Idle detection not supported on this platform
}

// ─── Windows: Win32 window subclass ─────────────────────────────────────────

#[cfg(target_os = "windows")]
mod platform_windows {
    use super::*;
    use tauri::Manager;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
        WTSUnRegisterSessionNotification,
    };
    use windows_sys::Win32::UI::Shell::{
        DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PBT_APMSUSPEND, WM_ENDSESSION, WM_NCDESTROY, WM_POWERBROADCAST,
        WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
    };

    const SUBCLASS_ID: usize = 0xB0A6_3E47;

    pub fn init_platform(app_handle: &tauri::AppHandle) -> Result<(), String> {
        let window = app_handle
            .get_webview_window("main")
            .ok_or_else(|| "no main window".to_string())?;
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as HWND;

        unsafe {
            if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) == 0 {
                return Err(format!(
                    "failed to register session notifications: {}",
                    std::io::Error::last_os_error()
                ));
            }

            if SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) == 0 {
                return Err(format!(
                    "failed to subclass main window: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }

        Ok(())
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        _ref_data: usize,
    ) -> LRESULT {
        match msg {
            WM_WTSSESSION_CHANGE if wparam == WTS_SESSION_LOCK as usize => {
                if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_LOCK {
                    lock_vault("screen-lock");
                }
            }
            WM_POWERBROADCAST if wparam == PBT_APMSUSPEND as usize => {
                if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_SLEEP {
                    lock_vault("suspend");
                }
            }
            WM_QUERYENDSESSION | WM_ENDSESSION => {
                if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_RESTART {
                    lock_vault("shutdown");
                }
            }
            WM_NCDESTROY => unsafe {
                let _ = WTSUnRegisterSessionNotification(hwnd);
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            },
            _ => {}
        }

        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

// ─── macOS: NSWorkspace + NSDistributedNotificationCenter ───────────────────

#[cfg(target_os = "macos")]
mod platform_macos {
    use super::*;
    use block2::RcBlock;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{
        NSDistributedNotificationCenter, NSNotification, NSOperationQueue, NSString,
    };
    use std::ptr::NonNull;

    pub fn init_platform(app_handle: &tauri::AppHandle) -> Result<(), String> {
        // Tauri's setup closure runs on the main thread on macOS.
        // NSWorkspace and NSNotificationCenter require main-thread access.
        let _ = app_handle; // used only for consistency with Windows signature

        unsafe {
            let workspace = NSWorkspace::sharedWorkspace();
            let nc = workspace.notificationCenter();
            let main_queue = NSOperationQueue::mainQueue();

            // Sleep / suspend notifications via NSWorkspace notification center.
            let sleep_events: &[(&str, &'static str)] = &[
                ("NSWorkspaceScreensDidSleepNotification", "screen-sleep"),
                ("NSWorkspaceWillSleepNotification", "system-sleep"),
            ];

            for &(name, reason) in sleep_events {
                let ns_name = NSString::from_str(name);
                let block: RcBlock<dyn Fn(NonNull<NSNotification>)> =
                    RcBlock::new(move |_notif: NonNull<NSNotification>| {
                        if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_SLEEP {
                            lock_vault(reason);
                        }
                    });
                let observer = nc.addObserverForName_object_queue_usingBlock(
                    Some(&*ns_name),
                    None,
                    Some(&main_queue),
                    &*block,
                );
                // Keep observer alive for the app's lifetime.
                std::mem::forget(observer);
            }

            // Power-off / restart notification.
            let poweroff_name = NSString::from_str("NSWorkspaceWillPowerOffNotification");
            let poweroff_block: RcBlock<dyn Fn(NonNull<NSNotification>)> =
                RcBlock::new(move |_notif: NonNull<NSNotification>| {
                    if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_RESTART {
                        lock_vault("shutdown");
                    }
                });
            let observer = nc.addObserverForName_object_queue_usingBlock(
                Some(&*poweroff_name),
                None,
                Some(&main_queue),
                &*poweroff_block,
            );
            std::mem::forget(observer);

            // Screen lock notification via NSDistributedNotificationCenter.
            // Fires on Cmd+Ctrl+Q, Apple menu → Lock Screen, hot corner.
            let dnc = NSDistributedNotificationCenter::defaultCenter();
            let lock_name = NSString::from_str("com.apple.screenIsLocked");
            let lock_block: RcBlock<dyn Fn(NonNull<NSNotification>)> =
                RcBlock::new(move |_notif: NonNull<NSNotification>| {
                    if LOCK_MODE_KIND.load(Ordering::Relaxed) == LOCK_MODE_ON_LOCK {
                        lock_vault("screen-lock");
                    }
                });
            let observer = dnc.addObserverForName_object_queue_usingBlock(
                Some(&*lock_name),
                None,
                Some(&main_queue),
                &*lock_block,
            );
            std::mem::forget(observer);
        }

        Ok(())
    }
}
