//! Windows settings for the game process that cannot be passed on its command line: power
//! throttling (EcoQoS) and the per-app GPU preference. Everything here is best effort: a failure
//! only costs performance, so it is logged by the caller and never stops a launch.

use std::path::Path;
use std::process::Child;

/// Registry key Windows reads for "Graphics preference" per executable (Settings → Display →
/// Graphics writes the same values).
#[cfg(windows)]
const GPU_PREFS_KEY: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
/// "High performance". `GpuPreference=1;` is power saving, 0 lets Windows decide.
#[cfg(windows)]
const HIGH_PERF_GPU: &str = "GpuPreference=2;";

/// Opts `child` out of power throttling: Windows then keeps it on performance cores at full clock
/// even while its window is in the background or minimized, and honours its timer resolution.
#[cfg(windows)]
pub fn disable_power_throttling(child: &Child) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Threading::{
        ProcessPowerThrottling, SetProcessInformation, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE,
    };

    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        // The bits named in the control mask are set to their value in the state mask: zero
        // means "never throttle" instead of "let Windows decide".
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    // SAFETY: the handle belongs to `child`, which outlives the call; `state` is a valid struct
    // of the size we pass.
    let ok = unsafe {
        SetProcessInformation(
            child.as_raw_handle() as _,
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn disable_power_throttling(_child: &Child) -> std::io::Result<()> {
    Ok(())
}

/// Asks Windows to run `exe` on the high-performance GPU (`prefer = true`), or takes that request
/// back (`prefer = false`). Taking it back only removes the exact value written here: a choice the
/// player made in the Windows settings stays.
#[cfg(windows)]
pub fn set_gpu_preference(exe: &Path, prefer: bool) -> std::io::Result<()> {
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    let key = wide(GPU_PREFS_KEY);
    let name = wide(&exe.display().to_string());

    // SAFETY: every pointer refers to a NUL-terminated buffer or a correctly sized output buffer
    // that lives until after the call.
    unsafe {
        let mut current = [0u16; 64];
        let mut size = std::mem::size_of_val(&current) as u32;
        let read = RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            current.as_mut_ptr().cast(),
            &mut size,
        );
        let ours = read == 0 && from_wide(&current) == HIGH_PERF_GPU;

        if prefer {
            if ours {
                return Ok(());
            }
            let value = wide(HIGH_PERF_GPU);
            let err = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            );
            if err != 0 {
                return Err(std::io::Error::from_raw_os_error(err as i32));
            }
        } else if ours {
            let err = RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            if err != 0 {
                return Err(std::io::Error::from_raw_os_error(err as i32));
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_gpu_preference(_exe: &Path, _prefer: bool) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}
