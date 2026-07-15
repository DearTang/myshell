## 目标
让升级时双击新安装包，旧版卸载步骤**静默执行**（不再弹卸载向导），从"3 关交互"降到"1 次下一步"。

## 根因（已用官方源码实证）
`installer.nsi` → `PageLeaveReinstall` → `reinst_uninstall` 块（官方模板 L341-355）调用旧版 `uninstall.exe` 时没加 `/S`，普通模式（双击）下旧卸载器弹出完整向导。`installerHooks` 够不着这行（不是宏插入点），必须用 `nsis.template` 接管整份模板。

## 改动（共 3 步，最小化）

### 1. 新建 `src-tauri/nsis/installer.nsi`
- 内容 = 官方 tauri-v2.11.2 模板原文（已下载到 `%TEMP%\tauri_installer_dev.nsi`，确认无 BOM——避开你之前踩过的 BOM 坑）
- **唯一改动**：`reinst_uninstall` 块的 `ExecWait '$R1' $0` 行（L354）改为带 `/S`：
  ```nsis
  StrCpy $R1 "$R1 /S _?=$4" ; append silent + uninstall directory  ← 加了 /S
  ```
- 文件头加注释块，记录：来源 tag、改了哪一行、升级 Tauri 时如何 re-sync（5 分钟 diff job）

### 2. 编辑 `src-tauri/tauri.conf.json`
在 `bundle.windows.nsis` 块加一行（与现有 `installerIcon: "icons/icon.ico"` 同样的相对路径约定，相对 src-tauri/）：
```json
"template": "nsis/installer.nsi"
```

### 3. 验证 + 文档
- `npm run tauri:build`（~10 分钟）确认模板能编译、安装包正常生成——验证 `/S` 改动没破坏模板语法
- 按项目 Doc-after-feature 规则：追加 `progress.md` 阶段条目、`RELEASE_NOTES_STAGING.md` 一行、`README.md` 更新日志同步

## 改动后的升级体验
双击新包 → 欢迎页 → PageReinstall 页（默认选中"卸载后安装"，**点一下下一步**）→ 旧版**后台静默卸载（无界面）** → 装新版 → 完成。

## 边界情况（已确认安全）
- **无旧版**：PageReinstall 早退，`reinst_uninstall` 不执行，`/S` 无影响
- **旧版正在运行**：静默卸载器内 `CheckIfAppIsRunning` 在 silent 模式直接 kill 进程，正常
- **卸载失败**：`PageLeaveReinstall` 后续的退出码检查（L361-374）保留不变，失败仍回退到选择页
- **保留 app data**：`/S` 下"删除应用数据"勾选框不显示，默认不删数据（升级期望行为）

## 维护成本（接管模板的代价）
今后 Tauri 升级（如 2.12）若改了 `installer.nsi`，需手动 re-sync：diff 新官方模板 vs 我们的副本，把 `/S` 那行重新应用上去。文件头注释会写清这一点。