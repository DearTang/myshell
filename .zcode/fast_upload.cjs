
// 直接通过 stdio 在单个 MCP 会话里完成所有上传
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const MCP_EXE = 'E:\\Program Files\\MyShell\\myshell-mcp.exe';
const connection = '135.32.56.70';

const deployPath = 'F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\sftpMonitor_deploy.tar.gz';
const offlinePath = 'F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\offline_packages_py37.tar.gz';
const chunkSize = 3000;

function fileChunks(filePath) {
    const raw = fs.readFileSync(filePath);
    const b64 = Buffer.from(raw).toString('base64');
    const chunks = [];
    for (let i = 0; i < b64.length; i += chunkSize) {
        chunks.push(b64.slice(i, i + chunkSize));
    }
    return { chunks, totalSize: raw.length };
}

const deploy = fileChunks(deployPath);
const offline = fileChunks(offlinePath);

console.log(`Deploy: ${deploy.chunks.length} chunks (${Math.round(deploy.totalSize/1024)}KB)`);
console.log(`Offline: ${offline.chunks.length} chunks (${Math.round(offline.totalSize/1024)}KB)`);

// Build command list
const commands = [];

// Step 1: Init
commands.push({ cmd: 'cd /home/ustc && rm -f sftpMonitor_deploy.tar.gz sftpMonitor_deploy.tar.gz.b64 offline_packages_py37.tar.gz offline_packages_py37.tar.gz.b64 && echo INIT_OK', label: 'init' });

// Step 2: Deploy chunks
deploy.chunks.forEach((chunk, idx) => {
    commands.push({ cmd: `cd /home/ustc && printf '%s' '${chunk}' >> sftpMonitor_deploy.tar.gz.b64`, label: `deploy_${idx+1}/${deploy.chunks.length}` });
});

// Step 3: Deploy decode
commands.push({ cmd: 'cd /home/ustc && base64 -d sftpMonitor_deploy.tar.gz.b64 > sftpMonitor_deploy.tar.gz && rm sftpMonitor_deploy.tar.gz.b64 && echo DEPLOY_OK', label: 'deploy_decode' });

// Step 4: Offline chunks
offline.chunks.forEach((chunk, idx) => {
    commands.push({ cmd: `cd /home/ustc && printf '%s' '${chunk}' >> offline_packages_py37.tar.gz.b64`, label: `offline_${idx+1}/${offline.chunks.length}` });
});

// Step 5: Offline decode
commands.push({ cmd: 'cd /home/ustc && base64 -d offline_packages_py37.tar.gz.b64 > offline_packages_py37.tar.gz && rm offline_packages_py37.tar.gz.b64 && echo OFFLINE_OK', label: 'offline_decode' });

console.log(`Total commands: ${commands.length}`);

// Start MCP server
const proc = spawn(MCP_EXE, [], { stdio: ['pipe', 'pipe', 'pipe'] });
let buf = [];
let initialized = false;
let cmdIdx = 0;
let results = [];
let currentId = 10;

proc.stdout.on('data', (data) => {
    const str = data.toString();
    buf.push(str);
    process.stdout.write(`[stdout] ${str}`);
    
    // Process complete JSON messages
    const allText = buf.join('');
    const lines = allText.split('\n');
    buf = [lines.pop()]; // keep incomplete line
    
    for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        try {
            const msg = JSON.parse(trimmed);
            if (msg.id === 1 && msg.result) {
                // Initialized
                initialized = true;
                console.log('\n[MCP initialized, starting upload...]');
                sendNextCommand();
            } else if (msg.result && msg.result.content) {
                // Got result
                try {
                    const result = JSON.parse(msg.result.content[0].text);
                    results.push(result);
                    process.stdout.write(`  -> exit=${result.exit_code} stdout="${(result.stdout||'').slice(0,60)}"\n`);
                    if (result.stdout && result.stdout.includes('_OK')) {
                        process.stdout.write(`  ✓ ${commands[cmdIdx-1].label}\n`);
                    }
                    sendNextCommand();
                } catch(e) {
                    sendNextCommand();
                }
            }
        } catch(e) {}
    }
});

proc.stderr.on('data', (data) => {
    process.stderr.write(`[stderr] ${data}`);
});

function sendJson(msg) {
    const data = JSON.stringify(msg) + '\n';
    proc.stdin.write(data);
    process.stdout.write(`[send] ${data.slice(0,80)}`);
}

function sendNextCommand() {
    if (cmdIdx >= commands.length) {
        console.log('\n=== Upload complete ===');
        proc.kill();
        return;
    }
    const c = commands[cmdIdx];
    cmdIdx++;
    process.stdout.write(`\n[${c.label}] ${c.cmd.slice(0,80)}`);
    sendJson({
        jsonrpc: '2.0', method: 'tools/call',
        params: { name: 'ssh_exec', arguments: { connection, command: c.cmd, timeout: 30 } },
        id: currentId++
    });
}

// Send initialize
sendJson({ jsonrpc: '2.0', method: 'initialize', params: { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'uploader', version: '1.0' } }, id: 1 });

setTimeout(() => {
    if (!initialized) {
        sendJson({ jsonrpc: '2.0', method: 'notifications/initialized' });
    }
}, 500);

// Wait for all to finish
setTimeout(() => {
    console.log('\n=== Timeout, killing MCP ===');
    console.log(`Commands sent: ${cmdIdx}/${commands.length}`);
    proc.kill();
}, 60000);

