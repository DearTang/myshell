
// 直接通过 stdio 直接调用 MCP 上传
const fs = require('fs');

const deployPath = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\sftpMonitor_deploy.tar.gz";
const offlinePath = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\offline_packages_py37.tar.gz";

function fileToB64Chunks(filePath, chunkSize = 3000) {
    const raw = fs.readFileSync(filePath);
    const b64 = raw.toString('base64');
    const chunks = [];
    for (let i = 0; i < b64.length; i += chunkSize) {
        chunks.push(b64.slice(i, i + chunkSize));
    }
    return chunks;
}

function printUploadCommandsForCopy(localPath, remoteName, chunks) {
    const shortName = localPath.split('\\').pop();
    console.log(`\n\n===== Upload ${shortName} =====`);
    // Cleanup
    console.log(`
      mcp__myshell__ssh_exec({
        command: 'cd /home/ustc && rm -f ${remoteName} ${remoteName}.b64',
        connection: '135.32.56.70'
      });
    `);
    // Send chunks
    chunks.forEach((chunk, idx) => {
        console.log(`
      mcp__myshell__ssh_exec({
        command: 'cd /home/ustc && printf \\'%s\\' \\'${chunk}\\'>
        connection: '135.32.56.70'
      });
    `.replace(/>$/, ` >> ${remoteName}.b64;`);
    });
    // Decode
    console.log(`
      mcp__myshell__ssh_exec({
        command: 'cd /home/ustc && base64 -d ${remoteName}.b64 > ${remoteName} && rm ${remoteName}.b64 && ls -lah ${remoteName}',
        connection: '135.32.56.70'
      });
    `);
}

const deployChunks = fileToB64Chunks(deployPath);
const offlineChunks = fileToB64Chunks(offlinePath);

console.log(`// DEPLOY: ${deployChunks.length} chunks`);
console.log(`// OFFLINE: ${offlineChunks.length} chunks\n\n`);

// Only output deploy first (small file first)
console.log("// ==================================");
console.log(" ========= DEPLOY PACKAGE ===============");
console.log("===================================");

deployChunks.forEach((chunk, i) => {
  console.log(`
mcp__myshell__ssh_exec({
    command: 'cd /home/ustc && printf '%s' '${chunk}' >> sftpMonitor_deploy.tar.gz.b64',
    connection: '135.32.56.70'
  });
`);
});
