//! Small Windows integrations: opening links and starting with Windows.

use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
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

/// The URL schemes of game servers.
const LINK_SCHEMES: [&str; 2] = ["ss14", "ss14s"];

fn scheme_key(scheme: &str) -> String {
    format!("Software\\Classes\\{scheme}")
}

/// What Windows runs for a link: our exe, with the link as the only argument.
fn link_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(format!("\"{}\" \"%1\"", exe.display()))
}

/// Sets a string value (`name = None` is the key's default value), creating the key if needed.
fn write_string(subkey: &str, name: Option<&str>, data: &str) -> Result<(), String> {
    let subkey = wide(subkey);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated strings and out pointers; null for the optional arguments.
    let rc = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(format!("Не удалось записать в реестр (код {rc})"));
    }
    let key = RunKeyGuard(key);
    let name_w = name.map(wide);
    let data_w = wide(data);
    // SAFETY: `data_w` is a NUL-terminated UTF-16 string; the size is in bytes.
    let rc = unsafe {
        RegSetValueExW(
            key.0,
            name_w.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data_w.as_ptr().cast(),
            (data_w.len() * 2) as u32,
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Не удалось записать в реестр (код {rc})"))
    }
}

struct RunKeyGuard(HKEY);

impl Drop for RunKeyGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegCreateKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

/// Reads a string value (`name = None` is the default value).
fn read_string(subkey: &str, name: Option<&str>) -> Option<String> {
    let subkey = wide(subkey);
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: valid NUL-terminated string and out pointer.
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut key) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let key = RunKeyGuard(key);
    let name_w = name.map(wide);
    let name_ptr = name_w.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());

    let mut size: u32 = 0;
    // SAFETY: asking for the size only.
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `buf` holds at least `size` bytes.
    let rc = unsafe {
        RegQueryValueExW(
            key.0,
            name_ptr,
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Is this launcher the one Windows opens `ss14://` and `ss14s://` links with?
pub fn links_registered() -> bool {
    let Ok(expected) = link_command() else {
        return false;
    };
    LINK_SCHEMES.iter().all(|s| {
        read_string(&format!("{}\\shell\\open\\command", scheme_key(s)), None).as_deref()
            == Some(expected.as_str())
    })
}

/// Makes this launcher the handler of `ss14://` and `ss14s://` links (for the current user),
/// or removes the registration. Opening a link only selects the server: the player still has
/// to press Play.
pub fn set_links_registered(enabled: bool) -> Result<(), String> {
    for scheme in LINK_SCHEMES {
        let base = scheme_key(scheme);
        if enabled {
            write_string(&base, None, &format!("URL:{scheme} Protocol"))?;
            write_string(&base, Some("URL Protocol"), "")?;
            write_string(
                &format!("{base}\\shell\\open\\command"),
                None,
                &link_command()?,
            )?;
        } else if links_registered() {
            // Only remove what is ours: it may belong to the official launcher now.
            let subkey = wide(&base);
            // SAFETY: valid NUL-terminated key path.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr()) };
        }
    }
    Ok(())
}


pub fn read_cpu_name() -> Option<String> {
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let subkey = wide(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
    let mut key: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey.as_ptr(), 0, KEY_READ, &mut key) };
    if rc != ERROR_SUCCESS {
        return std::env::var("PROCESSOR_IDENTIFIER").ok();
    }
    struct RegGuard(HKEY);
    impl Drop for RegGuard {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }
    let _guard = RegGuard(key);

    let val_name = wide("ProcessorNameString");
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegQueryValueExW(
            key,
            val_name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut u8,
            &mut size,
        )
    };
    if rc == ERROR_SUCCESS {
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len]).trim().to_string())
    } else {
        std::env::var("PROCESSOR_IDENTIFIER").ok()
    }
}

pub fn is_vulnerable_intel_cpu() -> bool {
    let Some(name) = read_cpu_name() else {
        return false;
    };
    if !name.contains("Intel") && !name.contains("GenuineIntel") {
        return false;
    }
    let affected = ["i7-13", "i9-13", "i7-14", "i9-14", "i5-13600K", "i5-14600K"];
    let excluded = ["HX", "H", "P", "U", "T"];
    let has_affected = affected.iter().any(|a| name.contains(a));
    let has_excluded = excluded.iter().any(|e| name.ends_with(e) || name.contains(&format!("{e} ")));
    has_affected && !has_excluded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_command_quotes_the_program_and_the_link() {
        let c = link_command().unwrap();
        assert!(c.starts_with('"') && c.ends_with(" \"%1\""));
    }

    #[test]
    fn registry_strings_roundtrip() {
        let key = format!("Software\\BebraLauncherTest\\{}", std::process::id());
        write_string(&key, None, "default value").unwrap();
        write_string(&key, Some("named"), "").unwrap();
        assert_eq!(read_string(&key, None).as_deref(), Some("default value"));
        assert_eq!(read_string(&key, Some("named")).as_deref(), Some(""));
        assert_eq!(read_string(&key, Some("missing")), None);

        let wide_key = wide(&key);
        // SAFETY: valid key path; removes only the test key made above.
        unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide_key.as_ptr()) };
        assert_eq!(read_string(&key, None), None);
    }

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
