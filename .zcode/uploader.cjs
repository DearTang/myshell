
// 分块上传脚本，base64 编码发送到服务器
const fs = require('fs');
const { spawnSync } = require('child_process');

const connection = "135.32.56.70";
const deployPath = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\sftpMonitor_deploy.tar.gz";
const offlinePath = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\offline_packages_py37.tar.gz";
const chunkSize = 3000; // 3KB per chunk (safe)

console.log('=== Uploader ===');
console.log(`deploy: ${deployPath}`);
console.log(`offline: ${offlinePath}\n`);

function fileToB64Chunks(filePath) {
    const raw = fs.readFileSync(filePath);
    const b64 = raw.toString('base64');
    const chunks = [];
    for (let i = 0; i < b64.length; i += chunkSize) {
        chunks.push(b64.slice(i, i + chunkSize));
    }
    return chunks;
}

function printUploadCommands(localPath, remoteName, chunks) {
    const shortName = localPath.split('\\').pop();
    console.log(`\n=== Upload ${shortName} ===`);
    console.log(`// Total: ${chunks.length} chunks`);
    
    // Step 1: Prepare on server
    console.log(`
      // Clear previous
      cd /home/ustc && rm -f ${remoteName}.b64 ${remoteName}
    `);
    
    // Step 2: Append chunks
    chunks.forEach((chunk, idx) => {
        console.log(`
          // Chunk ${idx+1}/${chunks.length}
          cd /home/ustc && printf '%s' '${chunk}' >> ${remoteName}.b64
        `);
    });
    
    // Step3: Decode
    console.log(`
      // Decode
      cd /home/ustc && base64 -d ${remoteName}.b64 > ${remoteName} && ls -lah ${remoteName}
      // Cleanup
      rm -f ${remoteName}.b64
    `);
}

const deployChunks = fileToB64Chunks(deployPath);
const offlineChunks = fileToB64Chunks(offlinePath);

printUploadCommands(deployPath, "sftpMonitor_deploy.tar.gz", deployChunks);
printUploadCommands(offlinePath, "offline_packages_py37.tar.gz", offlineChunks);
console.log('\n=== Finished generation ===');
