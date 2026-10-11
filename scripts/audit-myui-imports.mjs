// 审计：模板里用到的 myui 组件是否都真的 import 了。
//
// 为什么需要它：MyShell 没有 app.use(MyUI) 全局注册（见 src/main.ts），
// myui 组件必须逐个 SFC 显式 import。一旦漏掉，Vue **不会报错**，而是把
// <MyIconButton> 当成未知自定义元素渲染成 <myiconbutton>；此时组件上写的
// class 照常生效，于是按钮变成"有底色、有边框、没有内容"的空盒子。
// vue-tsc 也抓不到——Volar 无法确定某组件是否会被全局注册，未知标签一律放行。
//
// 曾因此静默坏掉 11 处（6 个文件）的 MyIconButton/MyButton，用户看到的是
// 设置页关闭 ✕ 只剩一个空方框。改动 SFC 后跑一遍本脚本可防复发。
//
//   node scripts/audit-myui-imports.mjs      # 从仓库根目录运行

import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve("src");
const files = [];
(function walk(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p);
    else if (e.name.endsWith(".vue")) files.push(p);
  }
})(ROOT);

// Collect every component name myui actually exports.
const idx = fs.readFileSync("node_modules/myui/dist/components/MyUI/index.d.ts", "utf8");
const exported = new Set();
for (const m of idx.matchAll(/declare const (My[A-Za-z0-9_]+)/g)) exported.add(m[1]);
for (const m of idx.matchAll(/\b(My[A-Za-z0-9_]+) as (My[A-Za-z0-9_]+)/g)) {
  exported.add(m[1]);
  exported.add(m[2]);
}
// also derive from the built chunk filenames (MyButton.vue.js -> MyButton)
for (const f of fs.readdirSync("node_modules/myui/dist/components/MyUI")) {
  const m = f.match(/^(My[A-Za-z0-9_]+)\.vue\.js$/);
  if (m) exported.add(m[1]);
}
for (const f of fs.readdirSync("node_modules/myui/dist/components/MyUI")) {
  const m = f.match(/^(My[A-Za-z0-9_]+)\.d\.ts$/);
  if (m) exported.add(m[1]);
}

const localImports = new Map(); // alias -> name
const problems = [];

for (const file of files) {
  const src = fs.readFileSync(file, "utf8");
  // Union ALL <script> blocks — some SFCs (e.g. FontField) have both an
  // options block and a <script setup> block, and setup-time imports live in
  // the second one. Taking only the first block yields false positives.
  let script = "";
  for (const m of src.matchAll(/<script[^>]*>([\s\S]*?)<\/script>/g)) script += m[1] + "\n";
  const tplMatch = src.match(/<template>([\s\S]*)<\/template>\s*(?:<style[\s\S]*)?$/);
  const tpl = tplMatch ? tplMatch[1] : "";

  // names available in template
  const avail = new Set();
  for (const m of script.matchAll(/import\s+(?:type\s+)?\{([^}]*)\}\s+from\s+["'][^"']+["']/g)) {
    for (const part of m[1].split(",")) {
      const t = part.trim().replace(/^type\s+/, "");
      if (!t) continue;
      const as = t.split(/\s+as\s+/);
      avail.add((as[1] || as[0]).trim());
    }
  }
  for (const m of script.matchAll(/import\s+([A-Za-z0-9_$]+)\s*(?:,|from)/g)) avail.add(m[1]);

  // component tags used in template
  const used = new Map(); // tag -> count
  const tplNoComment = tpl.replace(/<!--[\s\S]*?-->/g, "");
  for (const m of tplNoComment.matchAll(/<([A-Z][A-Za-z0-9_]*)[\s/>]/g)) {
    used.set(m[1], (used.get(m[1]) || 0) + 1);
  }

  const missing = [];
  for (const [tag, count] of used) {
    if (avail.has(tag)) continue;
    if (exported.has(tag)) missing.push(`${tag} x${count}`);
  }
  if (missing.length) problems.push([path.relative(ROOT, file), missing]);
}

console.log(`myui exported names detected: ${exported.size}`);
console.log(`scanned ${files.length} .vue files\n`);
if (!problems.length) console.log("OK — every myui component used in a template is imported.");
for (const [file, missing] of problems) {
  console.log(`${file}`);
  for (const m of missing) console.log(`    ${m}`);
}