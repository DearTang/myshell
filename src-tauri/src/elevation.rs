//! Process elevation (admin / root) status + self-restart as elevated.
//!
//! "Run as administrator" for LOCAL terminals is implemented by elevating the
//! whole app, not a single connection (decision recorded in progress.md): a
//! spawned local shell inherits MyShell's integrity level, and a medium-IL
//! process cannot attach a ConPTY to a high-IL shell — so the only robust way
//! to get an elevated local terminal is to run MyShell itself elevated.
//! Windows Terminal takes the same approach (it spawns a separate elevated
//! window rather than mixing integrity levels in one process).
//!
//! Non-Windows: `is_elevated` reports root via `geteuid()==0`; `restart_as_admin`
//! is a stub (re-launching as root needs an interactive sudo/polkit prompt and
//! is out of scope — the UI hides the button on those builds).

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::ffi::c_void;
    use std::mem;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
    use winapi::um::securitybaseapi::GetTokenInformation;
    use winapi::um::winnt::{TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        // zeroed() over Default — TOKEN_ELEVATION is a trivial #[repr(C)] { u32 }
        // and we only read one field, so all-zero is a valid starting state.
        let mut elevation: TOKEN_ELEVATION = mem::zeroed();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut c_void,
            mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid is a pure, always-safe POSIX query with no preconditions.
    unsafe { geteuid() == 0 }
}

/// Re-launch the current executable elevated. On Windows this calls
/// `ShellExecuteW` with the `"runas"` verb, which surfaces the UAC consent
/// dialog; the caller is then expected to exit the current (non-elevated)
/// process — the elevated instance runs independently. Returns an error if the
/// user dismisses UAC or the launch otherwise fails.
#[cfg(windows)]
pub fn restart_as_admin() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::shellapi::ShellExecuteW;
    use winapi::um::winuser::SW_SHOWNORMAL;

    let exe = std::env::current_exe()
        .map_err(|e| format!("获取程序路径失败: {}", e))?;

    // ShellExecuteW takes NUL-terminated UTF-16 strings.
    let verb: Vec<u16> = std::ffi::OsStr::new("runas")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: verb/file are valid NUL-terminated UTF-16 living in this scope;
    // the remaining args are null. Pointer borrows do not outlive the call.
    let hinst = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW returns a pseudo-HINSTANCE; values <= 32 are error codes.
    // 1223 = ERROR_CANCELLED (user dismissed the UAC prompt).
    if (hinst as isize) <= 32 {
        return Err(match hinst as isize {
            1223 => "已取消管理员授权".to_string(),
            code => format!("以管理员启动失败 (错误码 {})", code),
        });
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn restart_as_admin() -> Result<(), String> {
    Err("当前平台暂不支持以管理员重启".to_string())
}