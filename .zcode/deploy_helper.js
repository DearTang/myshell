
// 简单的 base64 上传辅助脚本，输出可复制的命令
const fs = require('fs');
const path = require('path');

const deployPackage = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\sftpMonitor_deploy.tar.gz";
const offlinePackage = "F:\\workProject\\pythonProject\\perceptualCenter\\sftpMonitor\\offline_packages_py37.tar.gz";

function chunkFile(file, chunkSize = 4000) {
    const raw = fs.readFileSync(file);
    const b64 = raw.toString('base64');
    const chunks = [];
    for (let i = 0; i < b64.length; i += chunkSize) {
        chunks.push(b64.slice(i, i + chunkSize));
    }
    return chunks;
}

console.log(`// 1. Deploy package (${Math.round(fs.statSync(deployPackage).size/1024)}KB):`);
const deployChunks = chunkFile(deployPackage);
console.log(`cd /home/ustc && rm -f /home/ustc/sftpMonitor_deploy.tar.gz.b64 /home/ustc/sftpMonitor_deploy.tar.gz;`);
deployChunks.forEach(chunk => {
    console.log(`printf '%s' '${chunk}' >> /home/ustc/sftpMonitor_deploy.tar.gz.b64;`);
});
console.log(`cd /home/ustc && base64 -d sftpMonitor_deploy.tar.gz.b64 > sftpMonitor_deploy.tar.gz && rm sftpMonitor_deploy.tar.gz.b64 && ls -lah sftpMonitor_deploy.tar.gz;`);

console.log('\n\n\n');

console.log(`// 2. Offline packages (${Math.round(fs.statSync(offlinePackage).size/1024)}KB):`);
const offlineChunks = chunkFile(offlinePackage);
console.log(`cd /home/ustc && rm -f /home/ustc/offline_packages_py37.tar.gz.b64 /home/ustc/offline_packages_py37.tar.gz;`);
offlineChunks.forEach(chunk => {
    console.log(`printf '%s' '${chunk}' >> /home/ustc/offline_packages_py37.tar.gz.b64;`);
});
console.log(`cd /home/ustc && base64 -d offline_packages_py37.tar.gz.b64 > offline_packages_py37.tar.gz && rm offline_packages_py37.tar.gz.b64 && ls -lah offline_packages_py37.tar.gz;`);
