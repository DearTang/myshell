/* Full config: custom buttons 退出/覆盖启动, default=覆盖启动 — mirrors the Rust config exactly. */
#define UNICODE
#define _UNICODE
#include <windows.h>
#include <commctrl.h>
#include <stdio.h>

#pragma comment(lib, "comctl32.lib")
#pragma comment(lib, "ole32.lib")

int main(void) {
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);

    TASKDIALOG_BUTTON buttons[] = {
        { 100, L"退出" },
        { 101, L"覆盖启动" },
    };

    TASKDIALOGCONFIG cfg = {0};
    cfg.cbSize = sizeof(cfg);
    cfg.dwCommonButtons = 0;
    cfg.pszWindowTitle = L"MyShell";
    cfg.pszMainInstruction = L"应用已在运行";
    cfg.pszContent = L"检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；退出则保持当前实例继续运行（本次启动已自动结束）。";
    cfg.cButtons = 2;
    cfg.pButtons = buttons;
    cfg.nDefaultButton = 101; /* 覆盖启动 = default */

    int btn = 0;
    /* Byte-level dump — compare with the Rust side. */
    unsigned char *p = (unsigned char *)&cfg;
    wprintf(L"cfg addr = 0x%p (align mod 8 = %zu)\n", &cfg, (size_t)&cfg % 8);
    wprintf(L"cfg bytes = ");
    for (int i = 0; i < (int)sizeof(cfg); i++) wprintf(L"%02X", p[i]);
    wprintf(L"\nbuttons addr = %p (array of 2)\n", (void *)buttons);
    HRESULT hr = TaskDialogIndirect(&cfg, &btn, NULL, NULL);
    wprintf(L"TaskDialogIndirect hr=0x%08lX clicked=%d\n", (unsigned long)hr, btn);
    return 0;
}
