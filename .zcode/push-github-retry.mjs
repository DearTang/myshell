import { execSync } from 'child_process';

// Retry git push to github through the flaky proxy, then fall back to direct.
const attempts = 8;
for (let i = 1; i <= attempts; i++) {
  for (const mode of ['proxy', 'direct']) {
    try {
      const cmd = mode === 'proxy'
        ? 'git push github main'
        : 'git -c http.proxy= -c https.proxy= push github main';
      console.log(`[attempt ${i}/${attempts} via ${mode}]`);
      const out = execSync(cmd, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000 });
      console.log(out);
      console.log('PUSH_OK');
      process.exit(0);
    } catch (e) {
      console.log(`  failed: ${(e.stderr || e.message).toString().split('\n').slice(-1)[0].slice(0, 160)}`);
    }
  }
  if (i < attempts) {
    console.log(`  sleeping 45s...`);
    execSync('timeout /t 45 /nobreak >nul', { shell: 'cmd.exe', stdio: 'ignore' });
  }
}
console.log('PUSH_EXHAUSTED');
process.exit(1);
