// 验证修复后的 stdout 提取逻辑（对照 App.tsx 修复版 v2）
function extractFixed(command, outputBuf, sentinel) {
  const sentinelRe = new RegExp(
    sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)"
  );
  const match = outputBuf.match(sentinelRe);
  if (!match) return { sentinelFound: false };
  let stdout = outputBuf.slice(0, match.index);

  stdout = stdout.replace(/\r\n/g, "\n").replace(/\r/g, "\n");

  const echoSentinel = "echo " + sentinel + ":$?";
  const echoIdx = stdout.lastIndexOf(echoSentinel);
  if (echoIdx >= 0) stdout = stdout.slice(0, echoIdx);

  stdout = stdout.replace(/\n+$/, "");
  const lastNl = stdout.lastIndexOf("\n");
  const lastLine = stdout.slice(lastNl + 1);
  if (lastLine && /[#$]\s*$/.test(lastLine)) {
    stdout = stdout.slice(0, lastNl);
  }

  let start = stdout.indexOf(command);
  if (start >= 0) {
    start += command.length;
    const lineEnd = stdout.indexOf("\n", start);
    stdout = lineEnd >= 0 ? stdout.slice(lineEnd + 1) : "";
  } else {
    const lines = stdout.split("\n");
    if (lines.length > 0 && command.startsWith(lines[0].trim())) lines.shift();
    stdout = lines.join("\n");
  }
  stdout = stdout.replace(/^\n+/, "").replace(/\n+$/, "");
  return { sentinelFound: true, exitCode: match[1], stdout };
}

const sentinel = "__MCP_DONE_abc123__";
let pass = 0, fail = 0;
function test(name, command, pty, expected) {
  const r = extractFixed(command, pty, sentinel);
  const ok = r.stdout === expected;
  ok ? pass++ : fail++;
  console.log(`${ok ? "PASS" : "FAIL"} ${name}  => ${JSON.stringify(r.stdout)}`);
  if (!ok) console.log(`        expected: ${JSON.stringify(expected)}`);
}

test("PS1提示符+whoami", "whoami",
  "[ustc@vm-297736 ~]$ whoami\r\nustc\r\necho " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "ustc");

test("完整PS1(含命令后提示符)", "whoami",
  "[ustc@vm-297736 ~]$ whoami\r\nustc\r\n[ustc@vm-297736 ~]$ echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "ustc");

test("无提示符", "whoami",
  "whoami\r\nustc\r\necho " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "ustc");

test("多行输出", "cat /etc/hostname",
  "cat /etc/hostname\r\nvm-297736.novalocal\r\necho " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "vm-297736.novalocal");

test("无回显", "whoami",
  "ustc\r\n" + sentinel + ":0\r\n",
  "ustc");

const complexCmd = 'echo "=== OS ===" && cat /etc/os-release';
const complexPty =
  '[ustc@vm-297736 ~]$ ' + complexCmd + ' 2>/dev/null | head -5\r\n' +
  '=== OS ===\r\nNAME="ctyunos"\r\nVERSION="2.0.1"\r\n' +
  "echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n";
test("复杂命令(管道+重定向,输出含#)", complexCmd + " 2>/dev/null | head -5", complexPty,
  '=== OS ===\nNAME="ctyunos"\nVERSION="2.0.1"');

// 边界：输出本身最后一行以 # 结尾（如注释行）——确保不被误当提示符剥离
test("输出末行含#但不剥离", "echo a#b",
  "echo a#b\r\na#b\r\necho " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "a#b");

// 边界：root 提示符 # 结尾
test("root提示符#", "whoami",
  "[root@host ~]# whoami\r\nroot\r\n[root@host ~]# echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "root");

// 边界：空输出
test("空输出", "true",
  "[ustc@vm-297736 ~]$ true\r\n[ustc@vm-297736 ~]$ echo " + sentinel + ":$?\r\n" + sentinel + ":0\r\n",
  "");

console.log(`\n${pass} passed, ${fail} failed`);
