# TaskDialogIndirect minimal repro — validates API availability + struct layout
# Mirrors the Rust ask_restart_or_quit() config exactly (buttons 100/101, default 101).
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public class TD {
    [StructLayout(LayoutKind.Sequential)]
    public struct TASKDIALOG_BUTTON {
        public int nButtonID;
        public IntPtr pszButtonText;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct TASKDIALOGCONFIG {
        public uint cbSize;
        public IntPtr hwndParent;
        public IntPtr hInstance;
        public uint dwFlags;
        public uint dwCommonButtons;
        public IntPtr pszWindowTitle;
        public IntPtr pszMainIcon;
        public IntPtr pszMainInstruction;
        public IntPtr pszContent;
        public uint cButtons;
        public IntPtr pButtons;
        public uint nDefaultButton;
        public uint cRadioButtons;
        public IntPtr pRadioButtons;
        public int nDefaultRadioButton;
        public IntPtr pszVerificationText;
        public IntPtr pszExpandedInformation;
        public IntPtr pszExpandedControlText;
        public IntPtr pszCollapsedControlText;
        public IntPtr pszFooterIcon;
        public IntPtr pszFooter;
        public IntPtr pfCallback;
        public IntPtr lpCallbackData;
        public uint cxWidth;
    }

    [DllImport("comctl32.dll", CharSet = CharSet.Unicode)]
    public static extern int TaskDialogIndirect(ref TASKDIALOGCONFIG cfg, out int btn, out int radio, out int verify);
}
'@

$sz = [Runtime.InteropServices.Marshal]::SizeOf([type][TD+TASKDIALOGCONFIG])
Write-Output "cbSize = $sz"

$title   = [Runtime.InteropServices.Marshal]::StringToHGlobalUni('MyShell')
$main    = [Runtime.InteropServices.Marshal]::StringToHGlobalUni('应用已在运行')
$content = [Runtime.InteropServices.Marshal]::StringToHGlobalUni('检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；退出则保持当前实例继续运行（本次启动已自动结束）。')
$quit      = [Runtime.InteropServices.Marshal]::StringToHGlobalUni('退出')
$overwrite = [Runtime.InteropServices.Marshal]::StringToHGlobalUni('覆盖启动')

$btnSz = [Runtime.InteropServices.Marshal]::SizeOf([type][TD+TASKDIALOG_BUTTON])
$btns = [Runtime.InteropServices.Marshal]::AllocHGlobal(2 * $btnSz)
[Runtime.InteropServices.Marshal]::WriteInt32($btns, 0, 100)
[Runtime.InteropServices.Marshal]::WriteIntPtr($btns, 8, $quit)
[Runtime.InteropServices.Marshal]::WriteInt32($btns, $btnSz, 101)
[Runtime.InteropServices.Marshal]::WriteIntPtr($btns, $btnSz + 8, $overwrite)

$cfg = New-Object TD+TASKDIALOGCONFIG
$cfg.cbSize = $sz
$cfg.pszWindowTitle = $title
$cfg.pszMainInstruction = $main
$cfg.pszContent = $content
$cfg.cButtons = 2
$cfg.pButtons = $btns
$cfg.nDefaultButton = 101

$btn = 0; $radio = 0; $verify = 0
$hr = [TD]::TaskDialogIndirect([ref]$cfg, [ref]$btn, [ref]$radio, [ref]$verify)
Write-Output ("hr = 0x{0:X8}  clicked = {1}" -f $hr, $btn)
