/* Canonical MSDN TaskDialogIndirect example in plain C.
   Compile: cl /W4 /LD... no — plain: cl /W4 test-taskdialog.c /link comctl32.lib ole32.lib user32.lib
   This isolates every Rust-side factor. */
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <commctrl.h>
#include <stdio.h>

int main(void) {
    HRESULT hr_co = CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    wprintf(L"CoInitializeEx hr=0x%08lX\n", (unsigned long)hr_co);
    wprintf(L"sizeof(TASKDIALOGCONFIG)=%zu\n", sizeof(TASKDIALOGCONFIG));

    TASKDIALOGCONFIG cfg = {0};
    cfg.cbSize = sizeof(cfg);
    cfg.hwndParent = NULL;
    cfg.hInstance = NULL;
    cfg.dwFlags = 0;
    cfg.dwCommonButtons = TDCBF_OK_BUTTON;
    cfg.pszWindowTitle = L"MyShell";
    cfg.pszMainInstruction = L"应用已在运行";
    cfg.pszContent = L"检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；退出则保持当前实例继续运行（本次启动已自动结束）。";
    cfg.nDefaultButton = IDOK;

    int btn = 0;
    HRESULT hr = TaskDialogIndirect(&cfg, &btn, NULL, NULL);
    wprintf(L"TaskDialogIndirect hr=0x%08lX clicked=%d\n", (unsigned long)hr, btn);
    return 0;
}
