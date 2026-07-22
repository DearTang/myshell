// 复现 App.tsx 第 491-517 行的 stdout 提取逻辑
function extract(command, outputBuf, sentinel) {
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
  if (lines.length > 0 && command.startsWith(lines[0].trim())) {
    lines.shift();
  }
  stdout = lines.join("\n").replace(/\n+$/, "");
  return { sentinelFound: true, exitCode: match[1], stdout: JSON.stringify(stdout) };
}

const sentinel = "__MCP_DONE_abc123__";

// 场景A：理想化的 PTY 输出（命令回显独占一行，无提示符）
const idealPty =
  "whoami\r\nustc\r\necho " + sentinel + ":$?\r\n" + sentinel + ":0\r\n";
console.log("场景A(理想,无PS1):", extract("whoami", idealPty, sentinel));

// 场景B：交互式 shell 带 PS1 提示符，提示符+命令回显在同一行
// 这是真实 PTY（ssh 进入交互式 bash）的实际回显
const realPty =
  "[ustc@vm-297736 ~]$ whoami\r\nustc\r\necho " +
  sentinel +
  ":$?\r\n" +
  sentinel +
  ":0\r\n";
console.log("场景B(PS1前缀,提示符+命令同行):", extract("whoami", realPty, sentinel));

// 场景C：完整真实——含命令后的提示符
const fullRealPty =
  "[ustc@vm-297736 ~]$ whoami\r\nustc\r\n[ustc@vm-297736 ~]$ echo " +
  sentinel +
  ":$?\r\n" +
  sentinel +
  ":0\r\n";
console.log("场景C(完整真实):", extract("whoami", fullRealPty, sentinel));

// 场景D：复杂命令，多行输出（用户实际的 OS 探测命令）
const complexCmd = 'echo "=== OS ===" && cat /etc/os-release';
const complexPty =
  '[ustc@vm-297736 ~]$ ' + complexCmd + ' 2>/dev/null | head -5\r\n' +
  "=== OS ===\r\nNAME=\"ctyunos\"\r\nVERSION=\"2.0.1\"\r\n" +
  "echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n";
console.log("场景D(复杂命令):", extract(complexCmd, complexPty, sentinel));
