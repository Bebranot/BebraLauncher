//! Small Windows integrations: opening links and starting with Windows.

use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "BebraLauncher";

/// Arguments the launcher is started with by Windows at login: it stays minimized.
pub const AUTOSTART_ARG: &str = "--autostart";

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Only plain web links are opened, and never through a shell.
pub fn is_safe_web_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !is_safe_web_url(url) {
        return Err("Можно открывать только ссылки http и https".into());
    }
    // `rundll32 url.dll,FileProtocolHandler <url>` hands the link to the default browser
    // without a shell, so nothing in the link can be interpreted as a command.
    std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

struct RunKey(HKEY);

impl RunKey {
    fn open(access: u32) -> Option<Self> {
        let mut key: HKEY = std::ptr::null_mut();
        let subkey = wide(RUN_KEY);
        // SAFETY: valid NUL-terminated string and out pointer.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
        (rc == ERROR_SUCCESS).then_some(Self(key))
    }
}

impl Drop for RunKey {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open` and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

pub fn autostart_enabled() -> bool {
    let Some(key) = RunKey::open(KEY_READ) else {
        return false;
    };
    let name = wide(RUN_VALUE);
    // SAFETY: asking only whether the value exists; all out pointers are null.
    unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) == ERROR_SUCCESS
    }
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let key = RunKey::open(KEY_READ | KEY_WRITE).ok_or("Не удалось открыть раздел автозапуска")?;
    let name = wide(RUN_VALUE);
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let command = format!("\"{}\" {AUTOSTART_ARG}", exe.display());
        let data = wide(&command);
        // SAFETY: `data` is a NUL-terminated UTF-16 string; the size is in bytes.
        let rc = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("Не удалось включить автозапуск (код {rc})"));
        }
    } else {
        // SAFETY: valid key and name. A missing value is fine: it is already off.
        unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_web_links_are_allowed() {
        assert!(is_safe_web_url("https://example.com/path?a=b&c=d"));
        assert!(is_safe_web_url("http://example.com"));
        assert!(!is_safe_web_url("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_safe_web_url("javascript:alert(1)"));
        assert!(!is_safe_web_url("ms-settings:"));
        assert!(!is_safe_web_url("https://a.com/ b"));
        assert!(!is_safe_web_url("https://a.com/\n"));
        assert!(!is_safe_web_url(&format!(
            "https://a.com/{}",
            "x".repeat(3000)
        )));
    }
}
