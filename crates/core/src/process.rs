//! Process checks the launcher needs (Windows).

/// Is there a live process `pid` whose executable is `image_path`?
///
/// A bare pid is not enough: Windows reuses them, and the content database remembers which program
/// owned a pid (`RunningClient.MainModule`). The comparison ignores case.
pub fn is_process_running_from(pid: u32, image_path: &str) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: plain Win32 calls; the handle is closed on every path and the buffer outlives the call.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return false;
        }
        String::from_utf16_lossy(&buf[..len as usize]).eq_ignore_ascii_case(image_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ourselves() {
        let exe = std::env::current_exe().unwrap();
        let exe = exe.to_string_lossy();
        // `current_exe` may carry a `\\?\` prefix that the process query does not use.
        let plain = exe.trim_start_matches(r"\\?\");
        assert!(is_process_running_from(std::process::id(), plain));
        assert!(is_process_running_from(
            std::process::id(),
            &plain.to_uppercase()
        ));
    }

    #[test]
    fn wrong_program_or_pid_is_not_running() {
        assert!(!is_process_running_from(
            std::process::id(),
            r"C:\definitely\not\this.exe"
        ));
        assert!(!is_process_running_from(0, "x"));
        assert!(!is_process_running_from(u32::MAX - 3, "x"));
    }

    #[test]
    fn a_finished_child_is_not_running() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let cmd = r"C:\Windows\System32\cmd.exe";
        child.wait().unwrap();
        assert!(!is_process_running_from(pid, cmd));
    }
}
