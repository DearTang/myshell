// Dynamic-call variant: LoadLibrary + GetProcAddress, and print which
// comctl32.dll actually got loaded. Isolates static-import binding issues.
use std::os::raw::{c_int, c_void};

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

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, size: u32) -> u32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> c_int;
}

fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn main() {
    unsafe { CoInitializeEx(std::ptr::null_mut(), 0x2) };

    let comctl = unsafe { LoadLibraryW(wide("comctl32.dll").as_ptr()) };
    let mut path_buf = [0u16; 512];
    let n = unsafe { GetModuleFileNameW(comctl, path_buf.as_mut_ptr(), 512) };
    let path: String = String::from_utf16_lossy(&path_buf[..n as usize]);
    println!("comctl32 module handle = {:p}", comctl);
    println!("comctl32 path = {}", path);

    let proc = unsafe { GetProcAddress(comctl, b"TaskDialogIndirect\0".as_ptr()) };
    println!("TaskDialogIndirect proc = {:p}", proc);
    if proc.is_null() {
        println!("PROC NOT FOUND — v5 loaded, manifest not effective");
        return;
    }

    type FnPtr = unsafe extern "system" fn(
        *const TASKDIALOGCONFIG,
        *mut c_int,
        *mut c_int,
        *mut c_int,
    ) -> c_int;
    let f: FnPtr = unsafe { std::mem::transmute(proc) };

    let title = wide("MyShell");
    let main_instruction = wide("应用已在运行");
    let content = wide("检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；退出则保持当前实例继续运行（本次启动已自动结束）。");
    let quit_label = wide("退出");
    let overwrite_label = wide("覆盖启动");

    let buttons = [
        TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: quit_label.as_ptr() },
        TASKDIALOG_BUTTON { nButtonID: 101, pszButtonText: overwrite_label.as_ptr() },
    ];

    let mut cfg: TASKDIALOGCONFIG = unsafe { std::mem::zeroed() };
    cfg.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    cfg.pszWindowTitle = title.as_ptr();
    cfg.pszMainInstruction = main_instruction.as_ptr();
    cfg.pszContent = content.as_ptr();
    cfg.cButtons = 2;
    cfg.pButtons = buttons.as_ptr();
    cfg.nDefaultButton = 101;

    let mut clicked: c_int = 0;
    // Byte-level dump of exactly what we hand to the API.
    let cfg_addr = &cfg as *const _ as usize;
    let bytes = unsafe { std::slice::from_raw_parts(&cfg as *const _ as *const u8, 176) };
    println!("cfg addr = 0x{:X} (align mod 8 = {})", cfg_addr, cfg_addr % 8);
    println!("cfg bytes = {}", bytes.iter().map(|b| format!("{:02X}", b)).collect::<String>());
    println!("buttons addr = {:p} (array of 2)", buttons.as_ptr());
    let hr = unsafe { f(&cfg, &mut clicked, std::ptr::null_mut(), std::ptr::null_mut()) };
    println!("hr = 0x{:08X} ({})  clicked = {}", hr, hr, clicked);
}
