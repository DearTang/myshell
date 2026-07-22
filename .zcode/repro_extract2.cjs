// 精确模拟 App.tsx exec_in_tab 的真实时序与 outputBuf 构成
// 验证根因：onSshOutput 在命令发送之后才订阅，快速命令的输出已被消费

// ====== 当前实现（有 bug）：先 send，后订阅 ======
function currentImpl(command, ptyStream) {
  const sentinel = "__MCP_DONE_abc123__";
  // 模拟异步：send 后 100ms，再 send sentinel
  // 真实 PTY 字节流按时间顺序到达。onSshOutput 是事件订阅，只捕获订阅之后的 ssh_output 事件。
  // 但这里 ptyStream 是完整字符串——我们需模拟"订阅时刻"的切分点。

  // 关键：listen() 是 async 的（返回 Promise）。在 await sshSend(command) 之后，
  // await sleep(100)，await sshSend(sentinel)，然后 await onSshOutput()。
  // 此时命令 + 输出 + sentinel 可能都已到达（快速命令），订阅只拿到 sentinel 行 + 提示符。

  // 模拟"订阅建立时，outputBuf 从空开始，只累积此后的字节"
  // 订阅建立时刻：所有 send 都完成了。对于快速命令，之后几乎无新字节（只剩末尾 \r\n）
  // 对于真实 ctyunos，命令很快（whoami < 50ms），100ms sleep 后输出已全部渲染。
  // 订阅后能拿到的：可能只剩 echo sentinel 的回显 + sentinel 结果行（如果它们在订阅后才到）

  // 真实情况：sentinel 命令也是在订阅前 send 的。但 ssh_output 事件的派发是异步的，
  // 后端 emit → 前端事件循环 → handler。订阅和事件派发在同一 JS 事件循环里竞争。
  // 如果 sentinel 字节在订阅 resolve 之前就 emit 了 → 丢失 → 超时。
  // 如果在之后 emit → 能匹配 sentinel 但 outputBuf 里没有命令输出 → stdout 空。

  // 这正好解释现象：exit_code 正确（sentinel 行匹配到了），stdout 空（命令输出在订阅前丢失）
  const outputBufAfterSubscribe = ""; // 订阅后才有的字节——快速命令可能为空或仅尾部
  return extractOutput(command, outputBufAfterSubscribe, sentinel);
}

function extractOutput(command, outputBuf, sentinel) {
  const sentinelRe = new RegExp(
    sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)"
  );
  const match = outputBuf.match(sentinelRe);
  if (!match) return { sentinelFound: false };
  let stdout = outputBuf.slice(0, match.index);
  const echoSentinel = "echo " + sentinel + ":$?";
  const echoIdx = stdout.lastIndexOf(echoSentinel);
  if (echoIdx >= 0) stdout = stdout.slice(0, echoIdx);
  const lines = stdout.split(/\r?\n/);
  if (lines.length > 0 && command.startsWith(lines[0].trim())) lines.shift();
  stdout = lines.join("\n").replace(/\n+$/, "");
  return { sentinelFound: true, exitCode: match[1], stdout: JSON.stringify(stdout) };
}

const sentinel = "__MCP_DONE_abc123__";

console.log("=== 当前实现（bug）：订阅前 send，订阅后 outputBuf 为空 ===");
// 情况1：订阅建立时，命令+sentinel 字节都已派发完，订阅后无新字节 → 超时
console.log("情况1(全丢失,超时):", currentImpl("whoami", ""));

// 情况2：订阅后仅捕获到 sentinel 结果行（echo sentinel 回显 + 结果在同一块）
// outputBuf = "echo __MCP_DONE_abc123__:$?\r\n__MCP_DONE_abc123__:0\r\n"
const tailOnly = "echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n";
const r2 = extractOutput("whoami", tailOnly, sentinel);
console.log("情况2(仅尾部sentinel):", r2);

console.log("\n=== 修复后实现：先订阅，后 send ===");
function fixedImpl(command, fullPty) {
  // 订阅在 send 之前建立，outputBuf 累积从订阅时刻起的所有字节（含命令输出）
  const r = extractOutput(command, fullPty, sentinel);
  return r;
}
const fullPty =
  "[ustc@vm-297736 ~]$ whoami\r\nustc\r\n[ustc@vm-297736 ~]$ echo " +
  sentinel +
  ":$?\r\n" +
  sentinel +
  ":0\r\n";
console.log("修复后(完整捕获):", fixedImpl("whoami", fullPty));
