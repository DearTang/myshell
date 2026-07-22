# 修复 ssh_exec 空输出 bug + 部署 sftpMonitor 到 135.32.56.70

## 第一部分：修复 MyShell ssh_exec 空输出 bug

### 根因（已用 Node 精确复现）
`src/App.tsx` 的 `runExec`（第 422-544 行）在 `show_in_gui: true` 时捕获 PTY 输出存在两个 bug：

**Bug 1（致命，导致 stdout 完全为空）—— 订阅时序错误**
```
当前顺序（第 455-465 行）：
  await sshSend(command + "\n");      // ① 先发命令
  await sleep(100);                    // ② 等 100ms
  await sshSend(`echo ${sentinel}:$?\n`); // ③ 发 sentinel
  ...
  const unlisten = await onSshOutput(...)  // ④ 最后才订阅！
```
对快速命令（whoami/echo/cat），命令的实际输出在 ④ 订阅建立前就已派发并渲染完毕。订阅后仅捕获到 sentinel 回显行 → 匹配到 sentinel（exit_code 正确）但 `outputBuf.slice(0, match.index)` 为空 → stdout 返回 `""`。
Node 复现：`outputBuf = "echo __MCP_DONE_xxx__:$?\r\n__MCP_DONE_xxx__:0\r\n"` → `{stdout: ""}`（与实际现象完全一致）。

**Bug 2（输出洁净度）—— PS1 提示符剥离失败**
交互式 shell（如这台 ctyunos 的 bash）PTY 输出形如 `[ustc@vm-297736 ~]$ whoami\r\nustc\r\n...`。现有首行剥离逻辑 `command.startsWith(lines[0].trim())`（第 513 行）对 `[ustc@vm-297736 ~]$ whoami` 这类带提示符前缀的行返回 false，无法剥离 → stdout 残留提示符前缀。

### 修复方案（仅改 `src/App.tsx`，第 449-520 行）

**修复 Bug 1**：调换顺序——**先订阅 `onSshOutput`，再 send 命令**。
- 把 `let outputBuf = ""; let done = false; let timedOut = false;` 和 `onSshOutput(...)` 订阅块整体上移到 `sshSend` 之前。
- timeout/cleanup 的 setInterval 和 setTimeout 在订阅建立后立即设置（保持原样）。
- `onSshOutput` 是 Tauri 事件订阅，`listen()` resolve 后即对所有后续 `ssh_output` 事件生效，先订阅能保证命令输出的每一个字节都被累积。

**修复 Bug 2**：改用命令文本定位回显边界，而非依赖首行 `startsWith`。
- 在 `outputBuf` 中查找命令文本首次出现的位置作为回显起点（`outputBuf.indexOf(command)`），从该位置之后开始提取（跳过回显行）。
- 若找不到（如 PTY 不回显），退化为当前行为。
- 保留现有的 `echo sentinel:$?` 行剥离（`lastIndexOf`）和尾部换行清理。

修复后预期：`whoami` → stdout = `"ustc"`（干净，无提示符）。

### 验证
1. `npm run test:ts`（tsc --noEmit）类型检查通过。
2. 重新加载 GUI 让前端代码生效（dev 模式 HMR 自动；release 模式需 `npm run tauri:build` 或重启 `tauri:dev`）。**届时会询问你当前是 dev 还是 release 运行**，以确定最短生效路径。
3. 用 `ssh_exec` 对 135.32.56.70 跑 `whoami` 验证 stdout 非空且无提示符污染。

---

## 第二部分：部署 sftpMonitor 到 135.32.56.70:/opt/py

### 环境（已探测）
- 系统 ctyunos 2.0.1（类 RHEL），Python 3.7.9，用户 ustc（wheel 组，可 sudo 切 root）。
- 部署目标 `/opt/py`，切 root 操作。

### 步骤
1. **本地准备**：把 `offline_packages_py37/*.whl` 打成 tar 包（与 deploy 包一并上传，供内网离线安装）。
2. **上传**：`sftp_upload` 上传 `sftpMonitor_deploy.tar.gz` 和离线 whl 包到 `/home/ustc/`（ssh 连接走 60018 端口；该连接是 ssh 类型，sftp_upload 可复用 ssh session）。
3. **切 root 部署**（ssh_exec）：
   - `sudo` 创建 `/opt/py/sftpMonitor`
   - 解压 deploy 包到该目录
   - 创建 venv：`python3 -m venv /opt/py/sftpMonitor/venv`
   - 离线安装依赖：`venv/bin/pip install --no-index --find-links=<whl目录> APScheduler PyYAML psycopg2-binary`
   - 配置 `config/config.yaml`（已有完整的数据库连接：`135.32.64.136:6543/sjgxpt`，直接用；如需调整日志路径再改）
4. **启动验证**：前台试跑 `venv/bin/python main.py`，确认无报错（数据库连接、日志读取、watcher 初始化），观察 banner 输出。
5. **MCP 功能测试**：用修复后的 `ssh_exec` 验证命令输出正常；`sftp_list` 验证文件浏览；确认整个部署链路通过 MCP 可操作。
6. （可选）配置 systemd service 开机自启。

### 风险与注意
- `sudo`/`systemctl`/`python3`/`pip3` 等命令在 MCP 命令规则黑名单中，会触发确认弹窗——届时会逐个提示你点"是"。
- 配置文件中的数据库密码会出现在命令里（用于写 config.yaml）——属敏感信息，我会用 heredoc 写文件而非明文回显。
- ssh_exec 修复前，部署步骤的输出读取依赖修复生效（这正是为何先修 bug）。

---

## 执行顺序
1. 修复 App.tsx（Bug1 + Bug2）→ test:ts 类型检查
2. 重新加载 GUI 生效 + 用 whoami 验证 ssh_exec 输出正常
3. 上传 deploy 包 + 离线 whl 包
4. 切 root 部署到 /opt/py/sftpMonitor
5. 启动验证 + MCP 全链路测试
6. 按项目规范更新文档（progress.md 阶段记录 + RELEASE_NOTES_STAGING.md 追加修复条目）