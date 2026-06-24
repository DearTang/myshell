#!/usr/bin/env node
/**
 * Publish a Gitee release for MyShell and attach the built NSIS installer.
 *
 * Usage:
 *   node scripts/publish-gitee-release.mjs <version> <notes-file> [asset-path]
 *
 *   version     version WITHOUT a leading "v", e.g. "1.4.6" (tag becomes v1.4.6)
 *   notes-file  Markdown file whose contents become the release body
 *   asset-path  optional; auto-detected as the newest *-setup.exe under
 *               src-tauri/target/release/bundle/nsis when omitted
 *
 * Token resolution (first non-empty wins):
 *   1. $GITEE_TOKEN environment variable
 *   2. .gitee-token file at repo root (gitignored — see .gitignore)
 *
 * Requires Node >= 18 (uses the global fetch + FormData + Blob). The repo
 * already targets modern Node, so no polyfill.
 *
 * API reference:
 *   - create release:  POST /v5/repos/{owner}/{repo}/releases
 *   - attach file:     POST /v5/repos/{owner}/{repo}/releases/{id}/attach_files
 *     (multipart/form-data — Gitee's release-asset mechanism differs from
 *      GitHub's upload_url; it uses a fixed attach_files endpoint)
 */
import { readFile, readdir, stat } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, basename } from "node:path";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OWNER = "argustang";
const REPO = "myshell";
const API = `https://gitee.com/api/v5/repos/${OWNER}/${REPO}`;

const [rawVersion, notesFile, assetOverride] = process.argv.slice(2);

if (!rawVersion || !notesFile) {
  console.error(
    "usage: node scripts/publish-gitee-release.mjs <version> <notes-file> [asset-path]"
  );
  process.exit(1);
}

// Strip a leading "v" if the caller passed one; tag is always v-prefixed.
const version = rawVersion.replace(/^v/i, "").trim();
const TAG = `v${version}`;

// ── 1. Resolve token ──────────────────────────────────────────────────────
function resolveToken() {
  if (process.env.GITEE_TOKEN) return process.env.GITEE_TOKEN.trim();
  const tokFile = join(ROOT, ".gitee-token");
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
  // Pick the most recently built installer (matches the version we just built).
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
      "未找到 Gitee token。设置 $GITEE_TOKEN 环境变量，或在仓库根创建 .gitee-token 文件（内含私人令牌）。"
    );
    process.exit(1);
  }

  const notes = await readFile(notesFile, "utf8");
  const asset = await resolveAsset().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });

  // ── 3. Create the release (tag is created from target_commitish=main) ──
  console.log(`=== 创建 Gitee release ${TAG} ===`);
  const createRes = await fetch(`${API}/releases`, {
    method: "POST",
    headers: { "Content-Type": "application/json;charset=UTF-8" },
    body: JSON.stringify({
      access_token: token,
      tag_name: TAG,
      name: TAG,
      body: notes,
      target_commitish: "main",
      prerelease: false,
    }),
  });
  const createJson = await createRes.json().catch(() => ({}));
  if (!createRes.ok || !createJson.id) {
    console.error("创建 release 失败:", JSON.stringify(createJson));
    process.exit(1);
  }
  const releaseId = createJson.id;
  console.log(`release id = ${releaseId}`);

  // ── 4. Upload the installer as a release attachment ───────────────────
  console.log(`=== 上传资产 ${basename(asset)} ===`);
  const fileBuf = await readFile(asset);
  const form = new FormData();
  form.append("access_token", token);
  form.append("file", new Blob([fileBuf]), basename(asset));
  const upRes = await fetch(`${API}/releases/${releaseId}/attach_files`, {
    method: "POST",
    body: form,
  });
  const upJson = await upRes.json().catch(() => ({}));
  if (!upRes.ok) {
    console.error("上传资产失败:", JSON.stringify(upJson));
    console.error(
      "release 已创建（含更新内容），但二进制未上传。可在网页端手动上传：" +
        `https://gitee.com/${OWNER}/${REPO}/releases/edit/${releaseId}`
    );
    process.exit(1);
  }
  console.log("上传完成");

  console.log(
    `=== 完成: https://gitee.com/${OWNER}/${REPO}/releases/tag/${TAG} ===`
  );
}

main().catch((e) => {
  console.error("发布异常:", e);
  process.exit(1);
});
