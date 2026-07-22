// 直接通过 stdio 调用 myshell-mcp.exe，测试 ssh_exec 走 GUI show_in_gui 路径
// 绕过 ZCode 的 MCP 客户端，验证 App.tsx 修复在真实链路中是否生效
const { spawn } = require('child_process');

const exe = 'E:\\Program Files\\MyShell\\myshell-mcp.exe';
const proc = spawn(exe, [], { stdio: ['pipe', 'pipe', 'pipe'] });

let buf = '';
let results = [];
proc.stdout.on('data', (data) => {
  buf += data.toString();
  // MCP stdio 用换行分隔 JSON（myshell-mcp 是 line-delimited）
  let idx;
  while ((idx = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (line) {
      try {
        const msg = JSON.parse(line);
        results.push(msg);
        if (msg.id === 2) {
          // 这是 ssh_exec 的响应
          console.log('=== ssh_exec 响应 ===');
          console.log(JSON.stringify(msg, null, 2));
        }
      } catch (e) {
        // 可能不是 JSON（日志输出等），忽略
      }
    }
  }
});

proc.stderr.on('data', (data) => {
  process.stderr.write('[mcp stderr] ' + data.toString());
});

function send(msg) {
  proc.stdin.write(JSON.stringify(msg) + '\n');
}

// 发送 initialize
send({
  jsonrpc: '2.0', method: 'initialize',
  params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'direct-test', version: '1.0' } },
  id: 1
});

setTimeout(() => {
  send({ jsonrpc: '2.0', method: 'notifications/initialized' });
}, 300);

// 发送 ssh_exec
setTimeout(() => {
  console.log('=== 发送 ssh_exec: whoami ===');
  send({
    jsonrpc: '2.0', method: 'tools/call',
    params: { name: 'ssh_exec', arguments: { connection: '135.32.56.70', command: 'whoami' } },
    id: 2
  });
}, 800);

// 等待响应后退出
setTimeout(() => {
  console.log('\n=== 所有响应 ===');
  console.log('共收到', results.length, '条消息');
  proc.kill();
  process.exit(0);
}, 20000);
