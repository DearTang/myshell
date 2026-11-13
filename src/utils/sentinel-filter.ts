/**
 * 终端输出流里的哨兵行过滤器。
 *
 * 有两类命令会往 PTY 注入一条带哨兵的 `echo`，用来把执行结果带回前端：
 *
 *  1. MCP `show_in_gui` 路径（useMcpBridge.ts）：`echo __MCP_DONE_<rand>__:$?`
 *     取回 exit code。
 *  2. 拖拽上传的 cwd 探针（TerminalPanel.vue）：`echo __MYSHLL_CWD_<rand>__$PWD`
 *     取回远端当前工作目录，好让上传落到「用户 cd 过去的那个目录」。
 *
 * 两条命令的回显行与结果行都会原样出现在终端输出流里，用户并不需要看到，
 * 所以这个过滤器在数据进 xterm 之前把它们整行删掉。cwd 探针的结果值
 * 经 `onCwdLine` 回调旁路取出——过滤与取值在同一个 pass 里完成，
 * 不会出现「值取到了但行没删干净」或反之的分叉。
 *
 * 关键约束（CRITICAL，别改）：
 * 只有「可能是哨兵」的不完整尾部行才允许缓存。普通内容（提示符、用户
 * 击键、交互输出）必须立即放行——否则任何没有换行符的输出都会卡在缓冲
 * 里，终端看起来像死了。判定方式沿用 MCP 时代的前缀 "__MCP"。
 */

/** 半行缓存判定用的前缀。两种哨兵共用，减小误判面。 */
const SENTINEL_PREFIX = "__MCP";
/** MCP 的 exit-code 哨兵。 */
const SENTINEL_NEEDLE = "__MCP_DONE_";
/**
 * 拖拽上传的 cwd 哨兵。
 *
 * 名字必须含 `__MCP` 前缀：半行缓存判定只看这个前缀（见本模块的
 * CRITICAL 约束），少了它，被 chunk 切开的探针行前半段会被当成普通内容
 * 立即放行，用户眼前就闪过半条 `echo __MYSH...`。
 */
export const CWD_NEEDLE = "__MCP_CWD_";

/**
 * 结果行的形状：`__MCP_CWD_<rand>__<路径>`。
 *
 * 必须与**回显行**区分：用户敲的（以及 PTY 回显的）命令是
 * `echo __MCP_CWD_<rand>__$PWD`，同样含 needle，但 needle 后面跟着
 * 的是 `$PWD` 字面量而不是路径。少了这层区分，回显行会被当成结果行，
 * 回调拿到 "$PWD" 并把文件传到名为 "$PWD" 的目录里。正则要求 needle 后
 * 紧跟随机串 + `__` + 非空内容，回显行的 `$PWD` 落不到这个位置。
 */
const CWD_RESULT_RE = /__MCP_CWD_[0-9a-z]+__(?!\$)([^\r\n]*)/;

export interface SentinelFilter {
  /** 喂一段原始 PTY 输出；返回要写进 xterm 的字节（整段都是哨兵时返回 null）。 */
  feed(data: Uint8Array): Uint8Array | null;
}

/**
 * @param onCwdLine 可选：每遇到含 cwd 哨兵的完整行就回调一次，参数为
 *   解析出的路径（空串时给 null）。调用方一般用 Promise 的 resolve。
 */
export function createSentinelFilter(
  onCwdLine?: (cwd: string | null) => void,
): SentinelFilter {
  let lineBuf = "";
  // 流式 UTF-8 解码：PTY 按 16ms / 16KiB 批量 flush，一个多字节字符
  // （中文）可能被切成两片。无状态 decode 会把前半段变成不可恢复的
  // U+FFFD；带 stream:true 的单实例解码器跨 chunk 保留那半个字符。
  const decoder = new TextDecoder("utf-8", { fatal: false });
  const encoder = new TextEncoder();

  const extractCwd = (line: string): string | null => {
    // 只认真实结果行（needle 后是路径，不是 `$PWD` 字面量）。
    // 回显行匹配不上 → 返回 null，不会把 "$PWD" 当成目录。
    const m = line.match(CWD_RESULT_RE);
    if (!m) return null;
    const cwd = m[1].trim();
    return cwd.length > 0 ? cwd : null;
  };

  return {
    feed(data: Uint8Array): Uint8Array | null {
      lineBuf += decoder.decode(data, { stream: true });
      const outLines: string[] = [];
      let start = 0;
      for (;;) {
        const nl = lineBuf.indexOf("\n", start);
        if (nl < 0) break; // 不完整行留在尾部
        const line = lineBuf.slice(start, nl + 1);
        if (line.includes(CWD_NEEDLE)) {
          // 只有结果行才回调：回显行（`echo __MCP_CWD_x__$PWD`）解析不出
          // 路径，若也回调就会用 null 提前终结等待者，真正的结果行随后
          // 到达时已经没人接了。删除则两种行都删（用户都不该看到）。
          const cwd = extractCwd(line);
          if (cwd !== null) onCwdLine?.(cwd);
        }
        if (!line.includes(SENTINEL_NEEDLE) && !line.includes(CWD_NEEDLE)) {
          outLines.push(line);
        }
        start = nl + 1;
      }
      const tail = lineBuf.slice(start);
      if (tail.includes(SENTINEL_PREFIX)) {
        // 可能是被 chunk 边界切开的哨兵行——缓存到下次。
        lineBuf = tail;
      } else {
        if (tail) outLines.push(tail);
        lineBuf = "";
      }
      if (outLines.length === 0) return null;
      return encoder.encode(outLines.join(""));
    },
  };
}
