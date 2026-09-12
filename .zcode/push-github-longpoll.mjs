import { execSync } from 'child_process';

// Long-poll: push commits to github (proxy first, direct fallback), then
// re-point the v2.14.0 tag to the release commit fcfdf66 once push succeeds.
const attempts = 20;
const sleepSec = 180;
const sleep = () => execSync(`timeout /t ${sleepSec} /nobreak >nul`, { shell: 'cmd.exe', stdio: 'ignore' });

const tryPush = (refspec, label) => {
  for (const mode of ['proxy', 'direct']) {
    const prefix = mode === 'direct' ? 'git -c http.proxy= -c https.proxy= ' : 'git ';
    try {
      const out = execSync(`${prefix}push github ${refspec}`, {
        encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000,
      });
      console.log(`[${label} via ${mode}] OK`);
      if (out.trim()) console.log(out.trim().split('\n').slice(-2).join('\n'));
      return mode;
    } catch (e) {
      console.log(`[${label} via ${mode}] ${(e.stderr || e.message).toString().split('\n').filter(Boolean).slice(-1)[0].slice(0, 140)}`);
    }
  }
  return null;
};

for (let i = 1; i <= attempts; i++) {
  console.log(`=== attempt ${i}/${attempts} ===`);
  const mode = tryPush('main', 'commits');
  if (mode) {
    const tagMode = tryPush('+fcfdf66:refs/tags/v2.14.0', 'tag re-point');
    console.log(tagMode ? 'ALL_DONE' : 'COMMITS_OK_TAG_PENDING');
    process.exit(0);
  }
  if (i < attempts) sleep();
}
console.log('PUSH_EXHAUSTED');
process.exit(1);
