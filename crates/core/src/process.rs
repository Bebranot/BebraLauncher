//! Process checks the launcher needs (Windows).

/// What we managed to learn about a pid.
///
/// `Unknown` is not `Gone`: a failed probe (no rights, a protected process, a very long image path)
/// says nothing about whether the process exists, so callers must not treat it as dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// There is no such process, or the pid now belongs to another program.
    Gone,
    /// The process exists and runs the expected executable.
    Alive,
    /// The process could not be checked.
    Unknown,
}

/// Is there a live process `pid` whose executable is `image_path`?
///
/// A bare pid is not enough: Windows reuses them, and the content database remembers which program
/// owned a pid (`RunningClient.MainModule`). The comparison ignores case.
#[cfg(windows)]
pub fn process_state(pid: u32, image_path: &str) -> ProcessState {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: plain Win32 calls; the handle is closed on every path and the buffer outlives the call.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // A pid that does not exist is a parameter error; "access denied" means it does exist
            // but is not ours to inspect.
            return match GetLastError() {
                ERROR_INVALID_PARAMETER => ProcessState::Gone,
                ERROR_ACCESS_DENIED => ProcessState::Unknown,
                _ => ProcessState::Unknown,
            };
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return ProcessState::Unknown;
        }
        if String::from_utf16_lossy(&buf[..len as usize]).eq_ignore_ascii_case(image_path) {
            ProcessState::Alive
        } else {
            ProcessState::Gone
        }
    }
}

/// Non-Windows builds have no process query here. `Unknown` is the honest answer: it must not be
/// read as "the process is gone".
#[cfg(not(windows))]
pub fn process_state(_pid: u32, _image_path: &str) -> ProcessState {
    ProcessState::Unknown
}

/// [`process_state`] as a plain yes/no answer. `false` also covers `Unknown`; use
/// [`process_state`] where a failed check must not be read as "gone".
pub fn is_process_running_from(pid: u32, image_path: &str) -> bool {
    process_state(pid, image_path) == ProcessState::Alive
}

/// Trims the process working set to minimize memory usage while the game is running.
#[cfg(windows)]
pub fn trim_working_set() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Non-Windows dummy for trimming process working set.
#[cfg(not(windows))]
pub fn trim_working_set() {}

#[cfg(all(test, windows))]
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
        assert_eq!(
            process_state(std::process::id(), plain),
            ProcessState::Alive
        );
    }

    #[test]
    fn wrong_program_or_pid_is_not_running() {
        assert!(!is_process_running_from(
            std::process::id(),
            r"C:\definitely\not\this.exe"
        ));
        assert_eq!(
            process_state(std::process::id(), r"C:\definitely\not\this.exe"),
            ProcessState::Gone,
            "the pid exists but runs another program"
        );
        assert!(!is_process_running_from(0, "x"));
        assert_eq!(process_state(0, "x"), ProcessState::Gone);
        assert!(!is_process_running_from(u32::MAX - 3, "x"));
        assert_eq!(process_state(u32::MAX - 3, "x"), ProcessState::Gone);
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
        assert_ne!(process_state(pid, cmd), ProcessState::Alive);
    }
}
