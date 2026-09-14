// ANSI 位置换算：按"可见字符数"回溯定位原始串的切割点（保留 ANSI 码）。
// 从旧 App.tsx 的 stripFromAnsiPosition 原样移植。
export function stripFromAnsiPosition(str: string, visibleIndex: number): string {
  let visible = 0;
  let i = 0;
  const len = str.length;
  while (i < len) {
    if (visible >= visibleIndex) break;
    const code = str.charCodeAt(i);
    // CSI: ESC [ ... 末字节(0x40-0x7E)
    if (code === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5b) {
      i += 2;
      while (i < len) {
        const c = str.charCodeAt(i);
        i++;
        if (c >= 0x40 && c <= 0x7e) break;
      }
      continue;
    }
    // OSC: ESC ] ... (BEL \x07 或 ST \x1b\\)
    if (code === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5d) {
      i += 2;
      while (i < len) {
        if (str.charCodeAt(i) === 0x07) {
          i++;
          break;
        }
        if (str.charCodeAt(i) === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5c) {
          i += 2;
          break;
        }
        i++;
      }
      continue;
    }
    // 其他 ESC 序列：ESC + 一字节
    if (code === 0x1b) {
      i += 2;
      continue;
    }
    visible++;
    i++;
  }
  return str.slice(0, i);
}

export interface DangerSegment {
  text: string;
  danger: boolean;
}

/**
 * 把命令串按危险片段切分（MCP 确认对话框渲染用）。
 * 匹配黑名单正则 + 危险字面模式（$()、反引号、写重定向 >）。
 * 返回分段数组，danger=true 的段用红色渲染。
 */
export function splitDangerSegments(command: string, blacklist: string[]): DangerSegment[] {
  type Range = [number, number];
  const ranges: Range[] = [];

  // 1. 危险字面模式（硬下限）
  for (let i = 0; i < command.length; i++) {
    if (command.startsWith("$(", i)) ranges.push([i, i + 2]);
    if (command[i] === "`") ranges.push([i, i + 1]);
    // 写重定向：> 或 >>，后面不是 /dev/null 或 &
    if (command[i] === ">") {
      let j = i + 1;
      if (command[j] === ">") j++;
      while (command[j] === " " || command[j] === "\t") j++;
      const rest = command.slice(j);
      if (!rest.startsWith("/dev/null") && !rest.startsWith("&")) {
        ranges.push([i, j]);
      }
    }
  }

  // 2. 黑名单正则匹配
  for (const pat of blacklist) {
    try {
      const re = new RegExp(pat, "gi");
      let m: RegExpExecArray | null;
      while ((m = re.exec(command)) !== null) {
        if (m[0].length > 0) {
          ranges.push([m.index, m.index + m[0].length]);
        }
        if (m.index === re.lastIndex) re.lastIndex++;
      }
    } catch {
      /* 非法正则跳过 */
    }
  }

  if (ranges.length === 0) return [{ text: command, danger: false }];

  // 合并重叠区间
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Range[] = [ranges[0]];
  for (let i = 1; i < ranges.length; i++) {
    const last = merged[merged.length - 1];
    if (ranges[i][0] <= last[1]) {
      last[1] = Math.max(last[1], ranges[i][1]);
    } else {
      merged.push(ranges[i]);
    }
  }

  const parts: DangerSegment[] = [];
  let pos = 0;
  for (const [start, end] of merged) {
    if (pos < start) parts.push({ text: command.slice(pos, start), danger: false });
    parts.push({ text: command.slice(start, end), danger: true });
    pos = end;
  }
  if (pos < command.length) parts.push({ text: command.slice(pos), danger: false });
  return parts;
}
