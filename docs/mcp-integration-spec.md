# MyShell MCP 接入规范

给**编写 MCP 客户端适配层**的人（以及让 AI 调用 MyShell 的人）。目标是让接入方一次就能发对请求，而不是靠反复试错。

> 运行时的 AI agent 读的是服务器通过 `initialize` 返回的 `instructions` 字段与各工具的 `inputSchema`；
> 本文件是给**人**看的同一套规则的完整版。两者内容一致，以服务器返回的为准。

---

## 一、最重要的一条：命令用 `command_b64` 传

### 问题

shell 命令嵌进 JSON 字符串时有**两层互不兼容的转义**：

```
JSON 字符串层  →  反斜杠、双引号必须转义
shell 层       →  单引号、$、反引号、花括号是元字符
```

两层叠在一起，只要有一个字符转义错了，字符串会被静默截断，客户端报出的是这种**没有信息量**的错误：

| 实际报错 | 真实原因 |
|---|---|
| `Unterminated string` | 命令里的 `"` 或 `\` 把 JSON 字符串提前截断 |
| `Invalid escape character $` | `\$` 不是合法 JSON 转义 |
| `Expected '}'` | shell 的 `{`/`}` 与 JSON 结构混淆 |

**这类错误由 MCP 客户端在请求发出前抛出，服务器根本收不到请求**——所以它不会出现在 MyShell 的日志里，服务端也无从帮助诊断。

### 规范

命令包含下列**任意一个**字符时，改用 `command_b64`：

```
"   '   \   $   `   {   }   |   ;   换行
```

`command_b64` 是命令 UTF-8 字节的标准 base64。base64 字符集只有 `A-Za-z0-9+/=`，**在 JSON 层和 shell 层都不需要任何转义**，这一类失败从根上消失。

```jsonc
// 不要这样（会被两层转义夹击）
{ "name": "ssh_exec",
  "arguments": { "connection": "prod-db",
                 "command": "grep -n 'HeartbeatThread\\|Controller' /x.conf" } }

// 这样
{ "name": "ssh_exec",
  "arguments": { "connection": "prod-db",
                 "command_b64": "Z3JlcCAtbiAnSGVhcnRiZWF0VGhyZWFkXHxDb250cm9sbGVyJyAveC5jb25m" } }
```

- 简单命令（`uptime`、`ls /var/log`）继续用 `command` 即可，**没有废弃**。
- 两个参数都给时，`command_b64` 优先。
- schema 用 `anyOf` 表达「二选一」，不是 `required: ["command"]` —— 若你的客户端做严格 schema 校验，请按 `anyOf` 实现。

### 怎么生成

```python
import base64, json
cmd = "grep -n 'HeartbeatThread\\|Controller' /x.conf"
args = {"connection": "prod-db",
        "command_b64": base64.b64encode(cmd.encode()).decode()}
```

### 另一条更好的路：别发命令

要读文件内容，用 **`ssh_read_text`**：路径作为普通 JSON 参数传入，由 MyShell 内部处理转义（优先 SFTP，无 SFTP 时回退 shell 并对路径做单引号转义）。**不要**再手拼 `cat` / `echo` / heredoc。

---

## 二、适配层应当传递结构化参数，而不是字符串

常见的错误形态是让 AI 把整个参数对象**序列化成字符串再塞进一个字段**：

```
content = '{"connection":"prod-db","command":"..."}'   // ← 不要
content = {"connection":"prod-db","command":"..."}     // ← 要这样
```

字符串化会引入**第三层**转义。如果宿主协议要求字符串，请在适配层**直接接收字符串化的 JSON 并用标准 JSON 解析器解析**，不要做手写拼接或正则替换。

---

## 三、超时有三层，别混为一谈

| 层 | 典型值 | 表现 | 应对 |
|---|---|---|---|
| 客户端接收超时 | 30–60s（**由你的客户端设定**） | `Request timeout ... stage: receive` | 远端命令可能仍在运行。改用 `ssh_run` |
| SSH 握手超时 | ~10s | `连接超时（10s 无响应）` | 网络/端口/服务问题；先 `test_connection` |
| 命令超时 | 60s 默认，最大 3600 | 结果含 `timed_out: true` | 远端进程**未被杀死**，见下 |

**命令超时时远端进程仍在运行**——返回里会带 `possible_remote_still_running: true`。此时不要盲目重试（会叠加第二份），应当：轮询它重定向到的日志文件，或用 `ssh_run` + `ssh_cancel`。

适配层的接收超时应当 **大于** 它传给 `ssh_exec` 的 `timeout`，否则你会在服务端返回前先放弃。

---

## 四、失败要按 `error_kind` 分支，不要匹配中文

每个失败结果都带机器可读的分类与下一步建议：

```jsonc
{ "content": [{ "type": "text", "text": "错误: ..." }],
  "isError": true,
  "error_kind": "permission_denied",
  "hint": "远端拒绝了该操作（权限/策略原因，不是确认弹窗被取消）..." }
```

| `error_kind` | 含义 | 适配层/agent 该做什么 |
|---|---|---|
| `permission_denied` | 远端权限或策略拒绝（sudo、polkit、dmesg） | 试 `sudo -n`（非交互，失败立即返回），或换有权限的连接 |
| `user_denied` | **用户点了拒绝** | 硬性停止，不要重试、不要绕路，向用户汇报后等待 |
| `timeout` | 超时 | 见上 |
| `busy` | 该连接上一条命令仍在跑 | `hint` 里有已运行秒数与最长等待；等待或改用 `ssh_run` |
| `connection_lost` | 会话断开 | 重试一次（通常自动重连）；仍失败则 `test_connection` |
| `connect_failed` | 连不上主机 | 先 `test_connection` |
| `vault_locked` | 保险库未解锁 | 提示用户在 MyShell 窗口输入主密码，然后重试**同一**工具 |
| `bad_request` | 参数错误 | 检查参数名/取值 |
| `not_found` | 远端路径不存在 | `sftp_list` 看实际内容 |
| `internal` | 其他 | 查 `mcp-error-<日期>.log` |

---

## 五、并发与串行

- 同一连接的终端**一次只能跑一条命令**；第二条 `ssh_exec` 返回 `busy`（含等待时间），**命令未发出，可安全重试**。
- **不同连接完全并行**；`ssh_status` / `ssh_cancel` 等状态查询不受阻塞影响（阶段 185 起服务端并发处理工具调用）。
- 需要批量在多台机器上跑同一命令：直接并发发多个 `ssh_exec`（各自不同连接）即可。

---

## 六、其余约定

- **返回值已剥离终端控制序列**（CSI 颜色、OSC 标题/超链接），无需自行清洗；中文与制表/换行保留。
- `ssh_exec` 结果含 `duration_ms`、`timed_out`、`truncated`、`command`（回显）、`exit_code_source`；`exit_code` 是远端进程退出码，管道命令给出的是最后一段的状态。
- `truncated: true` 表示输出被 `MCP_SSH_MAX_OUTPUT_BYTES`（默认 4 MiB）截断，请改为重定向到文件再取。
- **确认门不可绕过**：`ssh_exec` / `ssh_run` 按白名单/黑名单决定是否弹确认框；文件类工具（`sftp_upload` / `sftp_remove` / `sftp_rename` / `upload_project` / `download_project` / `zmodem_upload`）**一律**弹框。不要设计「让 agent 声明已获授权从而跳过」的参数——那是设计前提，不是缺陷。
- 日志：基础轨 `mcp-<日期>.log`，错误轨 `mcp-error-<日期>.log`（同目录，保留 7 天）。报障只需后者。

---

## 七、客户端接入自检清单

- [ ] 参数以**结构化对象**传递，不做二次字符串化
- [ ] 含引号/反斜杠/`$`/反引号/花括号/管道的命令走 `command_b64`
- [ ] schema 校验按 `anyOf` 接受 `command` 或 `command_b64`
- [ ] 读文件用 `ssh_read_text`，不手拼 `cat`
- [ ] 客户端接收超时 > 传入的 `timeout`
- [ ] 失败分支读 `error_kind`，不匹配中文文案
- [ ] `busy` 时按 `hint` 的秒数等待，或改 `ssh_run`，不做紧密重试
- [ ] `user_denied` 视为硬性停止


## 文件类工具的确认在 GUI 内渲染（阶段 187）

`sftp_upload` / `sftp_remove` / `sftp_rename` / `upload_project` / `download_project` / `zmodem_upload` 这六个**不可逆**工具的人工确认，此前由 MCP 进程弹 Windows 原生 `MessageBoxW`。当用户正在 MyShell 窗口里工作时，那个框会**藏在窗口后面**并抢走焦点 —— 对覆盖、删除这类操作，漏看一次框就等于漏掉一次确认，而 AI 会一直等下去。

现在这些确认由 MyShell 界面内的确认对话框渲染（与命令确认同一个组件）。因此：

- **接入方无需适配**，确认语义完全一致（允许 / 拒绝 / 本轮会话均允许）。
- 用户可能需要先切换到 MyShell 窗口才能看到弹窗。若 MyShell 完全不可用（未安装 / 已挂起），回退到原生弹窗。
- **180 秒无响应 = 拒绝**。这是刻意的：不可逆操作里「用户没说好」必须读作「不」。若读作「是」，则「一直等下去」反而变成无条件放行。

## 登录门禁的等待契约（阶段 187）

MyShell **不**在任何单个工具调用里等待主密码。原因是协议层的：多数 MCP 客户端在一个请求上 30 秒左右就会放弃，隐式等待会让客户端和用户同时失败。

契约如下：

1. 门禁失败时返回 `isError: true`、`error_kind: "vault_locked"`、以及 `vault_state`（`locked` / `not_initialized` / `gui_unavailable` / `ipc_unresponsive`）与一段 `hint`。`error_kind` 对所有状态恒定（调用方的修法永远是「让用户解锁后重试」），`vault_state` 只用于细分展示。
2. AI 调用 `vault_status`（**豁免门禁**，只返回 `gui_running` / `initialized` / `unlocked` 三个布尔，不泄漏任何连接元数据）轮询状态，约每 5 秒一次、最多约 2 分钟。
3. `vault_state` 变为 `unlocked` 后，AI 重试**同一个**工具。不要改用其他工具 —— 用只读探针替代真正的操作会改变语义（例如拿 `list_connections` 顶替一次上传，什么也没发生）。

| 你看到 | 你应该 |
|---|---|
| `error_kind: vault_locked`, `vault_state: "locked"` | 请用户在 MyShell 里输入主密码，`vault_status` 轮询到 `unlocked` 后重试原工具 |
| `error_kind: vault_locked`, `vault_state: "not_initialized"` | 请用户在 MyShell 中完成主密码初始化 |
| `error_kind: vault_locked`, `vault_state: "gui_unavailable"` | 请用户先打开 MyShell 桌面应用 |
| `error_kind: vault_locked`, `vault_state: "ipc_unresponsive"` | GUI 进程可能在挂起，请用户检查/重启 `myshell.exe` |
