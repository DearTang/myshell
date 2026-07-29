#!/usr/bin/env node
/**
 * Publish a GitHub release for MyShell and attach the built NSIS installer.
 *
 * Usage:
 *   node scripts/publish-github-release.mjs <version> <notes-file> [asset-path]
 *
 *   version     version WITHOUT a leading "v", e.g. "2.7.0" (tag becomes v2.7.0)
 *   notes-file  Markdown file whose contents become the release body
 *   asset-path  optional; auto-detected as the newest *-setup.exe under
 *               src-tauri/target/release/bundle/nsis when omitted
 *
 * Token resolution (first non-empty wins):
 *   1. $GITHUB_TOKEN environment variable
 *   2. .github-token file at repo root (gitignored — see .gitignore)
 *
 * Requires Node >= 18 (uses the global fetch + FormData + Blob).
 *
 * API reference:
 *   - create release:  POST /repos/{owner}/{repo}/releases
 *   - upload asset:    POST {upload_url} (from create response, strip {?...})
 */
import { readFile, readdir, stat } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, basename } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "DearTang";
const REPO = "myshell";
const API = `https://api.github.com/repos/${OWNER}/${REPO}`;

const MAX_ATTEMPTS = 4;
async function fetchWithRetry(url, init, label) {
  let lastErr;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    try {
      const res = await fetch(url, init);
      return res;
    } catch (err) {
      lastErr = err;
      if (attempt < MAX_ATTEMPTS) {
        const waitMs = 2000 * attempt;
        console.error(
          `  [${label}] 网络错误(${attempt}/${MAX_ATTEMPTS})，${waitMs}ms 后重试: ${err.message || err}`
        );
        await sleep(waitMs);
      }
    }
  }
  throw lastErr;
}

const [rawVersion, notesFile, assetOverride] = process.argv.slice(2);

if (!rawVersion || !notesFile) {
  console.error(
    "usage: node scripts/publish-github-release.mjs <version> <notes-file> [asset-path]"
  );
  process.exit(1);
}

const version = rawVersion.replace(/^v/i, "").trim();
const TAG = `v${version}`;

// ── 1. Resolve token ──────────────────────────────────────────────────────
function resolveToken() {
  if (process.env.GITHUB_TOKEN) return process.env.GITHUB_TOKEN.trim();
  const tokFile = join(ROOT, ".github-token");
  if (existsSync(tokFile)) return readFileSync(tokFile, "utf8").trim();
  return "";
}

// ── 2. Resolve the installer asset ────────────────────────────────────────
async function resolveAsset() {
  if (assetOverride) {
    if (!existsSync(assetOverride)) {
      throw new Error(`指定的资产路径不存在: ${assetOverride}`);
    }
    return assetOverride;
  }
  const nsisDir = join(ROOT, "src-tauri/target/release/bundle/nsis");
  if (!existsSync(nsisDir)) {
    throw new Error(
      `未找到安装包目录 ${nsisDir}。请先运行打包 (cargo tauri build)。`
    );
  }
  const entries = await readdir(nsisDir);
  const exes = entries.filter((f) => f.endsWith("-setup.exe"));
  if (exes.length === 0) {
    throw new Error(`目录下没有 *-setup.exe: ${nsisDir}`);
  }
  let newest = null;
  let newestMtime = 0;
  for (const f of exes) {
    const full = join(nsisDir, f);
    const mtime = (await stat(full)).mtimeMs;
    if (mtime > newestMtime) {
      newestMtime = mtime;
      newest = full;
    }
  }
  return newest;
}

async function main() {
  const token = resolveToken();
  if (!token) {
    console.error(
      "未找到 GitHub token。设置 $GITHUB_TOKEN 环境变量，或在仓库根创建 .github-token 文件。"
    );
    process.exit(1);
  }

  const notes = await readFile(notesFile, "utf8");
  const asset = await resolveAsset().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });

  const headers = {
    Authorization: `Bearer ${token}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
  };

  // ── 3. Create the release ─────────────────────────────────────────────
  console.log(`=== 创建 GitHub release ${TAG} ===`);
  const createRes = await fetchWithRetry(
    `${API}/releases`,
    {
      method: "POST",
      headers: { ...headers, "Content-Type": "application/json" },
      body: JSON.stringify({
        tag_name: TAG,
        name: TAG,
        body: notes,
        target_commitish: "main",
        draft: false,
        prerelease: false,
      }),
    },
    "create-release"
  );
  const createJson = await createRes.json().catch(() => ({}));
  if (!createRes.ok || !createJson.id) {
    console.error("创建 release 失败:", JSON.stringify(createJson));
    process.exit(1);
  }
  const releaseId = createJson.id;
  // upload_url comes as "https://uploads.github.com/repos/.../assets{?name,label}"
  const uploadUrl = (createJson.upload_url || "").replace(/\{[^}]*\}/, "");
  console.log(`release id = ${releaseId}`);

  // ── 4. Upload the installer asset ─────────────────────────────────────
  console.log(`=== 上传资产 ${basename(asset)} ===`);
  const fileBuf = await readFile(asset);
  let upRes;
  let lastUploadErr;
  for (let attempt = 1; attempt <= MAX_ATTEMPTS; attempt++) {
    try {
      upRes = await fetch(`${uploadUrl}?name=${encodeURIComponent(basename(asset))}`, {
        method: "POST",
        headers: {
          ...headers,
          "Content-Type": "application/octet-stream",
          "Content-Length": String(fileBuf.length),
        },
        body: fileBuf,
      });
      break;
    } catch (err) {
      lastUploadErr = err;
      if (attempt < MAX_ATTEMPTS) {
        const waitMs = 2000 * attempt;
        console.error(
          `  [upload] 网络错误(${attempt}/${MAX_ATTEMPTS})，${waitMs}ms 后重试: ${err.message || err}`
        );
        await sleep(waitMs);
      }
    }
  }
  if (!upRes) throw lastUploadErr;
  const upJson = await upRes.json().catch(() => ({}));
  if (!upRes.ok) {
    console.error("上传资产失败:", JSON.stringify(upJson));
    console.error(
      `release 已创建但二进制未上传。可手动上传：https://github.com/${OWNER}/${REPO}/releases/edit/${TAG}`
    );
    process.exit(1);
  }
  console.log("上传完成");

  console.log(
    `=== 完成: https://github.com/${OWNER}/${REPO}/releases/tag/${TAG} ===`
  );
}

main().catch((e) => {
  console.error("发布异常:", e);
  process.exit(1);
});
