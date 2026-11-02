// 界面字号快捷键：Ctrl+= 放大 / Ctrl+- 缩小 / Ctrl+0 复位。
//
// 焦点分流与 ompDesktop 一致：焦点在 xterm 终端里时改**终端字号**
// （TERMINAL_FONT_SIZE_*，10-22），否则改**界面字号**
// （UI_FONT_PX_*，12-18）。这样同一组键在两种上下文里都符合直觉，
// 也不会出现「在终端里按 Ctrl+= 把侧栏字撑大」的怪异行为。
//
// 只在 App.vue 挂一次。捕获阶段监听：xterm 自己会吞 keydown（它把
// 绝大多数按键交给 PTY），冒泡阶段拦不住，必须 capture。
import { ui, setUiFontPx, UI_FONT_PX_DEFAULT } from "@/store/ui";
import {
  terminalFontSize,
  setTerminalFontSize,
} from "@/composables/useTerminalFont";
import { TERMINAL_FONT_SIZE_DEFAULT } from "@/themes";

/** 焦点是否落在终端里。xterm 的真实输入元素是 .xterm-helper-textarea，
 *  但它可能被隐藏、焦点落在 .xterm 容器或 xterm-screen 上，
 *  所以用 closest 检查整棵终端子树。 */
function isTerminalFocused(): boolean {
  const el = document.activeElement;
  return !!el && typeof el.closest === "function" && !!el.closest(".xterm");
}

/** 按键 → 步进值。"=" 与 "+" 都要认：Ctrl+= 是不按 Shift 的写法，
 *  部分键盘布局会直接给出 "+"。 */
function stepOf(event: KeyboardEvent): number | null {
  const key = event.key;
  if (key === "=" || key === "+") return 1;
  if (key === "-" || key === "_") return -1;
  if (key === "0") return 0;
  return null;
}

export function installFontSizeShortcuts(): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    // Ctrl（Windows/Linux）或 Cmd（macOS），且不带 Alt —— Alt+= 留给输入法/系统
    const mod = event.ctrlKey || event.metaKey;
    if (!mod || event.altKey) return;

    const step = stepOf(event);
    if (step === null) return;

    const inTerminal = isTerminalFocused();
    event.preventDefault();
    event.stopPropagation();

    if (inTerminal) {
      const next =
        step === 0
          ? TERMINAL_FONT_SIZE_DEFAULT
          : terminalFontSize.value + step;
      setTerminalFontSize(next);
      return;
    }

    const next = step === 0 ? UI_FONT_PX_DEFAULT : ui.uiFontPx + step;
    // 边界外只做钳制，不写存储以外的事；钳制在 setUiFontPx 内部完成
    setUiFontPx(next);
  };

  window.addEventListener("keydown", onKeyDown, { capture: true });
  return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
}
