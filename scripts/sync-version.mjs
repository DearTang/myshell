#!/usr/bin/env node
/**
 * Single-source-of-truth version sync.
 *
 * `Cargo.toml` `[package] version` is the ONLY place the version is authored.
 * This script reads it and propagates it to the npm-side files
 * (`package.json` + `package-lock.json`) so they stay consistent without manual
 * edits. `tauri.conf.json` needs no version field at all — Tauri v2 reads it
 * straight from `Cargo.toml` when the field is omitted.
 *
 * Wired into `npm run build` (which `tauri build` runs via beforeBuildCommand),
 * so a release build always carries the Cargo.toml version everywhere. Also
 * runnable on its own via `npm run version:sync`.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function readJSON(p) {
  return JSON.parse(readFileSync(p, "utf8"));
}

function writeJSON(p, data) {
  writeFileSync(p, JSON.stringify(data, null, 2) + "\n");
}

// 1. Source of truth: Cargo.toml [package] version.
const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
const m = cargo.match(/^version\s*=\s*"([^"]+)"/m);
if (!m) {
  console.error("[sync-version] could not find `version = \"…\"` in Cargo.toml");
  process.exit(1);
}
const version = m[1];

let touched = false;

// 2. package.json
const pkgPath = join(root, "package.json");
const pkg = readJSON(pkgPath);
if (pkg.version !== version) {
  pkg.version = version;
  writeJSON(pkgPath, pkg);
  console.log(`[sync-version] package.json -> ${version}`);
  touched = true;
}

// 3. package-lock.json root version (the "" entry + top-level).
const lockPath = join(root, "package-lock.json");
const lock = readJSON(lockPath);
let lockChanged = false;
if (lock.version !== version) {
  lock.version = version;
  lockChanged = true;
}
if (lock.packages && lock.packages[""] && lock.packages[""].version !== version) {
  lock.packages[""].version = version;
  lockChanged = true;
}
if (lockChanged) {
  writeJSON(lockPath, lock);
  console.log(`[sync-version] package-lock.json -> ${version}`);
  touched = true;
}

if (!touched) {
  console.log(`[sync-version] already at ${version} (Cargo.toml is the source)`);
}