// 通过 MCP ssh_exec 上传文件：base64 分块 echo → 服务器端拼接解码
// 用法: node deploy_via_mcp.cjs <本地文件> <远程目标路径>
const { spawnSync } = require('child_process');
const fs = require('fs');

const localFile = process.argv[2];
const remotePath = process.argv[3];
const connection = process.argv[4] || '135.32.56.70';

if (!localFile || !remotePath) {
  console.error('用法: node deploy_via_mcp.cjs <本地文件> <远程路径> [连接名]');
  process.exit(1);
}

// 读取文件并 base64 编码
const raw = fs.readFileSync(localFile);
const b64 = Buffer.from(raw).toString('base64');
console.log(`[传输] ${localFile} -> ${remotePath}`);
console.log(`[传输] 原始 ${raw.length} 字节, base64 ${b64.length} 字符`);

// 分块大小：每块 4000 字符（避免命令行过长 + ssh_exec 的限制）
const CHUNK = 4000;
const chunks = [];
for (let i = 0; i < b64.length; i += CHUNK) {
  chunks.push(b64.slice(i, i + CHUNK));
}
console.log(`[传输] 分 ${chunks.length} 块, 每块 ${CHUNK} 字符`);

// 通过 MCP server (stdio) 调用 ssh_exec
function callSshExec(command) {
  return new Promise((resolve, reject) => {
    const exe = 'E:\\Program Files\\MyShell\\myshell-mcp.exe';
    const proc = spawnSync(exe, [], {
      input: [
        JSON.stringify({ jsonrpc: '2.0', method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'upload', version: '1.0' } }, id: 1 }),
        JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }),
        JSON.stringify({ jsonrpc: '2.0', method: 'tools/call', params: { name: 'ssh_exec', arguments: { connection, command, timeout: 30 } }, id: 2 }),
      ].join('\n'),
      encoding: 'utf8',
      timeout: 45000,
    });

    const lines = (proc.stdout || '').split('\n');
    for (const line of lines) {
      try {
        const msg = JSON.parse(line.trim());
        if (msg.id === 2 && msg.result && msg.result.content) {
          const text = msg.result.content[0].text;
          const result = JSON.parse(text);
          resolve(result);
          return;
        }
      } catch (e) {}
    }
    reject(new Error('MCP 无响应: ' + (proc.stderr || '').slice(0, 200)));
  });
}

(async () => {
  try {
    // 清空目标临时文件
    console.log('[传输] 初始化远程临时文件...');
    let r = await callSshExec(`rm -f /tmp/_upload.b64 && touch /tmp/_upload.b64 && echo INIT_OK`);
    if (r.stdout.indexOf('INIT_OK') < 0) throw new Error('初始化失败: ' + JSON.stringify(r));
    console.log('[传输] 初始化完成');

    // 分块追加
    for (let i = 0; i < chunks.length; i++) {
      process.stdout.write(`\r[传输] 上传块 ${i + 1}/${chunks.length}...`);
      // 用 printf 安全写入（避免 echo 的转义问题）
      r = await callSshExec(`printf '%s' '${chunks[i]}' >> /tmp/_upload.b64`);
      if (r.exit_code !== 0) throw new Error(`块 ${i + 1} 失败: ${r.stderr}`);
    }
    console.log('\n[传输] 所有块上传完成');

    // base64 解码到目标路径
    console.log('[传输] 服务器端解码...');
    r = await callSshExec(`base64 -d /tmp/_upload.b64 > '${remotePath}' && rm -f /tmp/_upload.b64 && echo DECODE_OK`);
    if (r.stdout.indexOf('DECODE_OK') < 0) throw new Error('解码失败: ' + JSON.stringify(r));

    // 验证大小
    r = await callSshExec(`stat -c '%s' '${remotePath}'`);
    const remoteSize = parseInt((r.stdout || '0').trim(), 10);
    console.log(`[传输] 远程文件大小: ${remoteSize} 字节 (本地 ${raw.length})`);
    if (remoteSize === raw.length) {
      console.log(`[传输] ✅ 成功! 大小匹配`);
    } else {
      console.log(`[传输] ⚠️ 大小不匹配!`);
      process.exit(1);
    }
  } catch (e) {
    console.error('[传输] ❌ 失败:', e.message);
    process.exit(1);
  }
})();
