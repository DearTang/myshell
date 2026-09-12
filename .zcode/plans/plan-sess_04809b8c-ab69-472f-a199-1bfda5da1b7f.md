# MyShell 审计问题整体修复计划

## 总体策略

按“先封住可利用边界，再补齐协议能力，再做纵深防御与数据保护”的顺序实施。所有安全判断以下沉 Rust 后端为准，前端只负责交互；保留当前工作区尚未提交的 Ubuntu 内存识别和 SSH 登录横幅修复，不触碰现有 `.zcode` 未跟踪测试文件，也不提交或推送。

## 阶段 1：修复 MCP 危险命令确认绕过（P1）

涉及：
- `src-tauri/src/command_rules.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/bin/myshell-mcp.rs`
- `src/App.tsx`
- `src/api.ts`

实施：
1. 将白名单从“整条命令任意位置命中即豁免”改为严格的完整命令/完整命令段匹配。
2. 增加 quote-aware 的控制符分段，识别换行、`;`、`&&`、`||`、管道等；每个段都必须独立安全，空段、解析不确定或任一危险段都要求确认。
3. 保留命令替换和写重定向硬底线，不允许白名单覆盖。
4. 移除 `App.tsx` 中复制的安全判定逻辑，新增后端 Tauri 判定命令或让 IPC 携带后端判定结果，使 GUI 与 headless MCP 共用同一个 Rust 策略源。
5. 添加绕过回归测试：`rm ...; grep ...`、`sudo ... && grep ...`、换行复合命令、危险段藏在管道中、带引号的分号、正常只读 grep/xargs 场景。

## 阶段 2：修复 Windows SFTP 递归下载路径逃逸（P1）

涉及：
- `src-tauri/src/sftp.rs`
- 视复用情况抽取到 `src-tauri/src/path_safety.rs` 并在 `lib.rs` 注册

实施：
1. 增加“远端名称只能形成一个本地文件名组件”的统一校验。
2. 所有平台拒绝空名称、`.`、`..`、NUL 和本地路径分隔含义；Windows 额外拒绝 `\ / : * ? " < > |`、盘符/UNC、尾随点或空格、`CON/PRN/AUX/NUL/COM*/LPT*` 等保留名。
3. 目录骨架和文件任务在加入队列前统一验证；不安全项跳过并在传输错误列表中明确报告，不做可能碰撞的静默改名。
4. 构造目标后做词法 containment 检查；创建目录/文件前检查已有路径组件和目标叶子不是符号链接或 Windows reparse point，避免 `File::create` 跟随既有链接越界。
5. 保持并发、进度和取消模型不变，并确保异常返回后 `transfer_cancels` 仍被清理。
6. 添加跨平台单元测试，并在 Windows 覆盖反斜杠 `..`、混合分隔符、盘符、UNC、保留设备名、尾点/尾空格、正常 Unicode 文件名。

## 阶段 3：升级 FTP 依赖并封堵 CRLF 注入（P1）

涉及：
- `src-tauri/Cargo.toml` / `Cargo.lock`
- `src-tauri/src/ftp.rs`
- `src-tauri/src/main.rs`
- `src/components/ConnectionDialog.tsx`

实施：
1. 将 `suppaftp` 升级到 `>=10.0.2` 的兼容稳定版本，按 v10 编译接口调整 tokio/rustls 类型和调用。
2. 不仅依赖上游：在 MyShell Rust 后端增加统一 FTP 参数校验，所有用户名、密码、目录、文件名、重命名源/目标进入 suppaftp 前拒绝 `\r`、`\n` 和 NUL。
3. 前端同步给出即时字段错误，但后端校验才是安全边界。
4. 为登录、列表、mkdir、remove、rename、上传、下载分别添加 CR/LF 拒绝测试。
5. 重新执行 `cargo audit`，要求 `RUSTSEC-2026-0271` 消失；RSA Marvin advisory 作为无修复上游风险单独保留。

## 阶段 4：补齐 FTP/FTPS 功能闭环

涉及：
- `src-tauri/src/ftp.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts`
- `src/components/SftpPanel.tsx`
- `src-tauri/src/bin/myshell-cli.rs`（在现有 CLI 架构允许的范围补齐或明确报错）

实施：
1. 基于 suppaftp v10 + rustls 实现 explicit FTPS 和 implicit FTPS；证书默认走系统/标准信任链，不增加“无条件忽略证书”开关。
2. 实现 `ftp_upload`、`ftp_download`、`ftp_cancel_transfer`，复用现有 `sftp_transfer_progress` / `sftp_transfer_done` 数据结构和取消表，避免前端重复实现传输 UI。
3. FTP 下载支持当前 UI 宣称的文件与目录递归下载；本地目标复用阶段 2 的路径安全校验。
4. `SftpPanel` 的上传、下载、取消全部按 `source` 分发，不再把 FTP session id 发送给 SFTP 命令。
5. 明确覆盖主动/被动模式、代理、FTP、explicit FTPS、implicit FTPS 的连接和基本操作测试；无法自动联调的协议矩阵记录为人工验收项。

## 阶段 5：将密码二次验证下沉到 Rust（P2）

涉及：
- `src-tauri/src/main.rs`
- `src-tauri/src/vault.rs`
- `src/api.ts`
- `src/components/PasswordVerifyDialog.tsx`
- `src/components/ConnectionDialog.tsx`

实施：
1. 不采用可复用的长期前端标志；将“验证主密码 + 解密并返回指定连接密码”合并为单个后端命令，普通密码和代理密码分别有明确接口。
2. 后端复用现有 KDF、verifier 和 lockout 逻辑；密码错误计入现有失败/锁定策略，成功后仅返回本次请求对应的秘密。
3. 删除或收紧当前只需 DEK 即可调用的 reveal 命令，确保任何 WebView 调用者都不能绕过二次验证。
4. 前端对话框只收集本次主密码，调用后立即清空；增加正确密码、错误密码、锁定、连接不存在、vault 未解锁测试。

## 阶段 6：加固 GUI localhost IPC（P2）

涉及：
- `src-tauri/src/main.rs`
- `src-tauri/src/bin/myshell-mcp.rs`

实施：
1. 抽取单连接处理函数，接受循环不再被 `exec_in_tab` 等长请求串行阻塞。
2. 每条 NDJSON 请求设置固定上限（建议 1 MiB）；超过上限在 JSON 解析和令牌验证前立即关闭或返回统一错误。
3. 保留 5 秒读写超时，并增加并发连接上限，超过上限快速拒绝，防止本地线程/内存耗尽。
4. 对 action、command、connection id 和 timeout 增加字段长度/范围限制。
5. 保留 127.0.0.1、随机 32 字节令牌和常量时间比较；同用户可读 token 文件仍作为架构残余风险记录，后续若改 named pipe 再单独立项。
6. 增加超长行、慢连接、无 token、错误 token、并发请求和正常 `vault_status/open_connection/exec_in_tab` 测试。

## 阶段 7：修复 MCP 外部工具配置覆盖与非原子写（P2）

涉及：
- `src-tauri/src/mcp_tools.rs`
- 可抽取共用原子写 helper

实施：
1. 已存在配置解析失败时立即报错，绝不回退为空对象。
2. 校验目标父节点必须是 JSON object；类型不符时不覆盖。
3. 修改前创建可恢复备份；写入采用同目录临时文件、flush/sync、平台适配的安全替换，并清理失败临时文件。
4. 添加 Claude、Opencode、ZCode 三种结构的写入/删除测试：保留其他键、非法 JSON 不改文件、错误节点类型不改文件、写入失败原文件完整。

## 阶段 8：保护命令历史、快捷命令和 AI 对话内容（P2/隐私）

涉及：
- `src-tauri/src/db.rs`
- `src-tauri/src/main.rs`
- `src-tauri/src/ai.rs`
- `src-tauri/src/lib.rs`
- `src/api.ts` 及相关调用

实施：
1. 为 `command_history.command`、`quick_commands.command`、`ai_conversations.content` 增加 AES-GCM 密文字段；沿用现有 DEK 和连接字段加解密 helper。
2. 在 vault 解锁后的迁移流程中，将旧明文事务性迁移为密文；迁移成功后清空旧明文占位字段，保留 schema 向后兼容所需结构。
3. 新写入只存密文，读取必须持有 DEK；数据库错误或 vault 锁定时失败关闭。
4. 提供“禁用命令历史/按连接禁用”设置，避免不需要历史的用户产生敏感数据。
5. 增加旧库迁移、重复迁移幂等、错误密钥、密文篡改、CRUD 和删除清理测试。

## 阶段 9：验证、文档与发布缓冲

1. 运行：
   - `git diff --check`
   - `npx tsc --noEmit`
   - `npm run build`
   - `cargo fmt --check`
   - `cargo check`
   - `cargo test`
   - `npm audit --registry=https://registry.npmjs.org`
   - `cargo audit`（在线失败则明确标注并用缓存复核）
2. 补充针对本次漏洞的回归测试，不能只依赖现有 44 项测试。
3. 按仓库约定直接更新：
   - `progress.md` 新阶段及五问检查
   - `README.md` 功能能力与安全说明
   - `RELEASE_NOTES_STAGING.md` 分别记录安全修复、FTP/FTPS 新增和隐私加固
4. 输出仍需人工验证的矩阵：Windows 恶意 SFTP 文件名、真实 FTP/explicit FTPS/implicit FTPS、GUI MCP 确认框和密码查看流程。
5. 不执行 commit、push 或发布；当前工作区原有未提交文件保持不被误纳入。

## 实施拆分原则

这是跨 Rust 核心、Tauri IPC、React、SQLite schema 和协议依赖的多阶段改造。实施时每完成一个阶段先运行其定向测试，再进入下一阶段；若 suppaftp v10 的 FTPS API 或现有数据库迁移约束与预期不同，会优先保持“安全失败”，不会为了兼容而恢复明文 FTP 注入面或保留明文历史。