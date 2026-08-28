// Minimal TaskDialogIndirect repro — completely independent of myshell.
// Same struct layout & config as main.rs ask_restart_or_quit().
// rustc 1.60+ target x86_64-pc-windows-msvc; link comctl32 via #[link].

use std::os::raw::{c_int, c_void};

const BTN_QUIT: c_int = 100;
const BTN_OVERWRITE: c_int = 101;

#[repr(C)]
struct TASKDIALOG_BUTTON {
    nButtonID: c_int,
    pszButtonText: *const u16,
}

#[repr(C)]
union TaskDialogIcon {
    hMainIcon: *mut c_void,
    pszMainIcon: *const u16,
}

#[repr(C)]
union TaskDialogFooterIcon {
    hFooterIcon: *mut c_void,
    pszFooterIcon: *const u16,
}

#[repr(C)]
struct TASKDIALOGCONFIG {
    cbSize: u32,
    hwndParent: *mut c_void,
    hInstance: *mut c_void,
    dwFlags: u32,
    dwCommonButtons: u32,
    pszWindowTitle: *const u16,
    mainIcon: TaskDialogIcon,
    pszMainInstruction: *const u16,
    pszContent: *const u16,
    cButtons: u32,
    pButtons: *const TASKDIALOG_BUTTON,
    nDefaultButton: c_int,
    cRadioButtons: u32,
    pRadioButtons: *const TASKDIALOG_BUTTON,
    nDefaultRadioButton: c_int,
    pszVerificationText: *const u16,
    pszExpandedInformation: *const u16,
    pszExpandedControlText: *const u16,
    pszCollapsedControlText: *const u16,
    footerIcon: TaskDialogFooterIcon,
    pszFooter: *const u16,
    pfCallback: *const c_void,
    lpCallbackData: *mut c_void,
    cxWidth: u32,
}

#[link(name = "comctl32")]
extern "system" {
    fn TaskDialogIndirect(
        pTaskConfig: *const TASKDIALOGCONFIG,
        pnButton: *mut c_int,
        pnRadioButton: *mut c_int,
        pfVerificationFlagChecked: *mut c_int,
    ) -> c_int;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> c_int;
}

const COINIT_APARTMENTTHREADED: u32 = 0x2;

fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn main() {
    // Hypothesis: TaskDialogIndirect needs COM on the calling thread;
    // without it the API returns E_INVALIDARG (not CO_E_NOTINITIALIZED).
    let hr_init = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
    println!("CoInitializeEx hr = 0x{:08X} (S_OK=0, S_FALSE=1 already-inited)", hr_init);

    println!("cbSize = {}", std::mem::size_of::<TASKDIALOGCONFIG>());

    let title = wide("MyShell");
    let main_instruction = wide("应用已在运行");
    let content = wide("检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；退出则保持当前实例继续运行（本次启动已自动结束）。");
    let quit_label = wide("退出");
    let overwrite_label = wide("覆盖启动");

    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: BTN_QUIT,
            pszButtonText: quit_label.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            nButtonID: BTN_OVERWRITE,
            pszButtonText: overwrite_label.as_ptr(),
        },
    ];

    // Bisect #2: zero the whole struct first (Rust struct literals leave
    // inter-field padding as garbage; commctrl may validate it).
    let mut cfg: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    cfg.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    cfg.dwCommonButtons = 0x0001; // TDCBF_OK_BUTTON
    cfg.pszWindowTitle = title.as_ptr();
    cfg.pszMainInstruction = main_instruction.as_ptr();
    cfg.pszContent = content.as_ptr();
    cfg.nDefaultButton = 1; // IDOK

    let mut clicked: c_int = 0;
    let hr = unsafe {
        TaskDialogIndirect(
            &cfg,
            &mut clicked,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    println!("hr = 0x{:08X} ({})  clicked = {}", hr, hr, clicked);
}
