// MCP ↔ GUI 桥：监听 MCP 服务器的 open_connection / exec_in_tab /
// screenshot_terminal 命令。
// 从旧 App.tsx 的 mcp-gui-command useEffect 原样移植（sentinel 三层完成策略、
// 每连接执行互斥锁 + 陈旧锁自愈、GUI 确认对话框），语义零改动。
import { watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "myui";
import {
  checkCommandConfirmation,
  checkCommandDangerReasons,
  mcpConfirmResult,
  mcpExecResult,
  onSshOutput,
  saveScreenshot,
  showInFolder,
  sshSend,
} from "../api";
import { connectionsStore } from "../store/connections";
import { sessions, connect, reconnectOne, getTab, getTerminal } from "../store/sessions";
import { showMcpConfirm, ui } from "../store/ui";
import { captureTerminalToDataUrl } from "../utils/screenshot";
import { stripFromAnsiPosition } from "../utils/ansi";

interface McpGuiCommand {
  action: string;
  connection_id: string;
  tab_type?: string;
  focus_existing?: boolean;
  request_id?: string;
  command?: string;
  timeout?: number;
  /** MCP 侧已获会话授权（用户此前选过「本轮会话均允许」）→ 跳过确认弹窗 */
  session_allowed?: boolean;
  /** 调用方进程标识（myshell-mcp 的 pid）：exec 通道按 (连接, pid) 隔离终端
   *  标签 —— 每个 MCP 客户端进程一条独立持久 shell，多进程不串台。 */
  client_pid?: number;
  /** `mcp_confirm`（不可逆文件操作确认）：被请求的操作名称，如「上传文件（可能覆盖）」 */
  operation?: string;
  /** `mcp_confirm`：目标连接名，用于弹窗标题 */
  connection_name?: string;
  /** `mcp_confirm`：完整详情（路径、目标对象），展示给用户判断影响面 */
  detail?: string;
  /** `exec_task_progress`：MCP 后台任务（ssh_run）的阶段文案。null = 清除角标 */
  label?: string | null;
}

// 每连接执行锁：连接 id → 加锁时刻。交互式 PTY 串行执行，上一条没跑完时
// 拒绝注入下一条（避免命令字节交错 / sentinel 串台）。
//
// 陈旧锁自愈阈值不是固定值：MCP 侧允许 AI 传 timeout（默认 60s，最大 3600s），
// 固定 60s 会在长命令仍在跑时把锁强释，第二条命令随即注入同一条 PTY —— 正是
// 锁要防的 sentinel 串台。阈值 = max(MCP_LOCK_STALE_MS, 本请求 timeout + 10s)。
const mcpExecLocks = new Map<string, number>();
const MCP_LOCK_STALE_MS = 60_000;

/**
 * 已确认 TERM 齐备的会话集合（sessionId）。
 *
 * 为什么需要缓存：探针是一次远端往返（发 `echo $TERM` 等回显）。gui 通道的
 * 每个 MCP 客户端进程复用同一条持久 shell，所以「这个会话的 TERM 已经对了」
 * 在整条会话里都成立 —— 不缓存就等于给每次 ssh_exec 都加一次往返，而那正是
 * 反馈里「一次调用约 6 秒就被切断」最不该花的预算。
 */
const termDeclaredSessions = new Set<string>();

/** 探针标记：只认这一串，避免把用户自己 echo 的内容误判成探针结果。 */
const TERM_PROBE_MARK = "__MYSHLL_TERM__";

/**
 * 确保远端 shell 的 TERM 已显式声明为 xterm-256color。
 *
 * 背景：gui 通道的 PTY 由 ssh.rs 以 `xterm-256color` 申请（ssh.rs request_pty），
 * 但 TERM 变量是否真的出现在远端环境里取决于 sshd 的 `AcceptEnv` 配置 —— 多数
 * 默认配置不收。结果是「有 PTY 但没有 TERM」的中间态：提示符正常，而依赖
 * terminfo 的 `clear` 行为不确定，采集输出里还带着
 * `TERM environment variable not set.`。
 *
 * 判据（刻意保守）：探针读到非空值就认为齐备，一律不覆盖用户自己设的 TERM
 * （可能是 dumb、screen-256color 等有意选择）。只在缺失/为空时才导出。
 */
async function ensureTermDeclared(sessionId: string): Promise<void> {
  if (termDeclaredSessions.has(sessionId)) return;
  try {
    const current = await probeTerm(sessionId);
    if (current) {
      termDeclaredSessions.add(sessionId);
      return;
    }
    // 只在缺失时导出。export 对当前 shell 生效，且该 shell 被同一 MCP 客户端
    // 进程复用，所以后续命令都看得到。
    await sshSend(sessionId, "export TERM=xterm-256color\n");
    termDeclaredSessions.add(sessionId);
  } catch {
    // 探针失败不阻断执行：TERM 缺失只是让部分命令行为不确定，不是致命错误。
    // 标记为已处理，避免每条命令都重试一次注定失败的探针。
    termDeclaredSessions.add(sessionId);
  }
}

/** 发一次 `echo <mark>:$TERM` 并用一次性监听取回该行的值。 */
//
// 刻意用 new Promise 而非 Promise.withResolvers：本项目 tsconfig 的 lib 是
// ES2021（见 tsconfig.json），没有 withResolvers 类型；为它抬 lib 会波及其
// 其他文件的类型解析，不值的当。
//
// 取值的两个坑（探针脚本 F:/tmp/term-probe-check.mjs 实测抓到）：
//   1. 命令**回显行**（`echo __MYSHLL_TERM__:$TERM`）也含标记，且排在结果行
//      之前。只按「标记:」匹配会先命中回显、把字面量 `$TERM` 当成已设值，
//      于是永远跳过导出 —— TERM 缺失恰恰静默延续。判据加一条：值不得以 `$`
//      开头（那是未展开的变量名，不是值）。
//   2. 结果行尾常带 readline 的 ANSI（`\x1b[m\x1b[K`），原样返回会把
//      `xterm-256color` 判成非空但脏的值。先剥 ANSI 再取。
function probeTerm(sessionId: string): Promise<string> {
  return new Promise<string>((resolve) => {
    let settled = false;
    let stop: UnlistenFn | null = null;
    const finish = (v: string) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      stop?.();
      resolve(v);
    };
    // 探针有自己的预算：3 秒拿不到就当作「读不到」，与命令超时无关。
    const timer = setTimeout(() => finish(""), 3000);
    void (async () => {
      try {
        stop = await onSshOutput(sessionId, (data) => {
          const chunk = new TextDecoder("utf-8", { fatal: false }).decode(data, {
            stream: true,
          });
          const value = parseTermProbeValue(chunk);
          if (value !== null) finish(value);
        });
        await sshSend(sessionId, `echo ${TERM_PROBE_MARK}:$TERM\n`);
      } catch {
        finish("");
      }
    })();
  });
}

/**
 * 从一段 PTY 输出里解析 TERM 探针的值。
 *
 * 返回 null = 还没看到结果行（调用方继续等）；返回空串 = 结果行在、TERM 未
 * 设置（需要导出）；返回非空 = 远端已有 TERM（一律不覆盖）。
 */
function parseTermProbeValue(chunk: string): string | null {
  const re = new RegExp(`${TERM_PROBE_MARK}:([^\\r\\n]*)`);
  let m: RegExpExecArray | null;
  let rest = chunk;
  // 逐个候选扫过去：第一个可能是命令回显（值是 `$TERM`），结果行排在它后面。
  while ((m = re.exec(rest)) !== null) {
    const raw = stripAnsiTail(m[1]);
    // 未展开的变量名不是值 —— 那是回显行，继续找结果行
    if (!raw.startsWith("$")) return raw;
    rest = rest.slice(m.index + m[0].length);
  }
  return null;
}

/// 剥掉行尾的 CSI 序列（readline 重绘留下的 `\x1b[m` / `\x1b[K` 等）。
/// 只处理行尾：值本身不可能含合法 ANSI，这样不必引入完整的状态机。
function stripAnsiTail(s: string): string {
  return s.replace(/(?:\x1b\[[0-9;]*[a-zA-Z])+$/, "");
}

// 进程级终端标签注册表："连接id:客户端pid" → tabId。exec_in_tab 据此复用
// 本进程自己的标签；标签被用户关掉后从这里清除（startMcpBridge 内 watch）。
// MCP 进程重启换 pid → 自然落新键、开新标签，旧标签留作孤儿由用户手关
//（断开≠退出，不自动关）。
const mcpProcessTabs = new Map<string, string>();

// 截图新建标签轮询的活动句柄。三条正常出口（连上 / 连接失败 / 轮询超时）
// 各自 clearInterval 并 delete；桥卸载（MCP 客户端断开）这条路径只有这里
// 能覆盖 —— 不清的话 interval 会带着 sessions.tabs 引用转到应用结束。
// 本项目 tsconfig 走 DOM lib，setInterval 返回 number（NodeJS.Timeout 不在
// 这个工程的环境里），直接用具名类型，不用 ReturnType<typeof setInterval>。
const activeScreenshotPolls = new Set<number>();

/**
 * 把 MCP 正在使用的标签带到前台——但只在用户没有别的活动标签时。
 *
 * 早期实现无条件写 `sessions.activeTabId = tab.id`，本意是「让用户看见 AI
 * 在哪个终端跑命令」。实际后果是每次 MCP 调用（含 5s 一次的探活）都把用户
 * 正在看的标签切走：底栏 ServerInfoPanel 跟着 activeTab 跳，于是两台机器的
 * CPU/内存/磁盘数据来回闪，用户看到的就是「系统信息不停切换会话」。
 *
 * MCP 执行本身不依赖活动态——输出采集走 `onSshOutput(sessionId, …)` 事件，
 * runExec 拿的也是 sessionId。所以切页只是可见性提示，不该抢用户的操作焦点。
 *
 * 规则：当前没有活动标签（首帧、刚关完标签）时才聚焦；用户已经在看某个
 * 标签时不打扰。若将来需要「AI 操作提示」，应做标签条上的角标而不是抢焦点。
 */
function focusTabIfIdle(tabId: string): void {
  if (sessions.activeTabId === null) sessions.activeTabId = tabId;
}

export async function startMcpBridge(): Promise<UnlistenFn> {
  const unlisten = await listen<McpGuiCommand>("mcp-gui-command", (event) => {
    const { action, connection_id } = event.payload;

    // ── mcp_confirm：不可逆文件操作的人工确认（GUI 内渲染）──
    //
    // 必须在 connection_id 守卫之前：文件类操作的 payload 不带 connection_id
    // （main.rs 的 mcp_confirm 分支只发 action/request_id/operation/
    // connection_name/detail），守卫先返回会让弹窗永不渲染，MCP 侧空等 180s
    // 后按 deny 收场 —— 用户看到的就是「任务卡在等待确认，但没弹框」。
    if (action === "mcp_confirm" && event.payload.request_id) {
      const { request_id, operation, connection_name, detail } = event.payload as {
        request_id: string;
        operation: string;
        connection_name: string;
        detail: string;
      };
      // The listener callback is synchronous (Tauri events), so the dialog
      // promise is chained rather than awaited: `showMcpConfirm` parks its
      // resolver until the user clicks, and the decision goes back over IPC.
      // dialog 的 command 字段渲染全文并做危险片段高亮；detail 一并传入，
      // 用户能看到完整对象与目标路径。
      void showMcpConfirm(
        `${operation}\n\n${detail}`,
        connection_name || "MyShell",
        [],
      ).then((decision) => mcpConfirmResult(request_id, decision));
      return;
    }

    if (!connection_id) return;

    // ── exec_task_progress：MCP 后台任务（ssh_run）的阶段角标 ──
    //
    // 为什么值得做：ssh_run 的存在理由就是「客户端单次调用预算 ~30s，而任务本身
    // 要跑几分钟」。AI 侧轮询 ssh_status 能看到进度，用户侧却只看到一台安静的
    // MyShell —— 完全看不出有个 5 分钟的安装在跑。角标把阶段写到该连接的标签
    // 标题上，是成本最低的诚实答案（不伪造终端流，也不抢焦点）。
    //
    // label 为 null 表示清除（任务跑完 / 失败 / 被 ssh_cancel）。空串等同 null：
    // 一个空角标在标题上只是噪音。
    if (action === "exec_task_progress") {
      const label = event.payload.label;
      const text = typeof label === "string" ? label.trim() : "";
      if (text) {
        sessions.taskBadges[connection_id] = text;
      } else {
        delete sessions.taskBadges[connection_id];
      }
      return;
    }

    const config = connectionsStore.connections.find((c) => c.id === connection_id);
    if (!config) {
      console.warn(`[mcp-gui] connection not found: ${connection_id}`);
      if (action === "exec_in_tab" && event.payload.request_id) {
        mcpExecResult(event.payload.request_id, { ok: false, error: "未找到连接" });
      }
      if (action === "screenshot_terminal" && event.payload.request_id) {
        mcpExecResult(event.payload.request_id, { ok: false, error: "未找到连接" });
      }
      return;
    }

    // ── screenshot_terminal：截取该连接终端标签页的当前画面并保存 ──
    //
    // 与 exec_in_tab 的分工：exec 要往 PTY 写命令，screenshot 只读画面，
    // 所以它没有每连接执行锁，也没有确认门（截图不是危险操作）。捕获直接
    // 走 terminalRegistry 里的 xterm 实例（getTerminal(sessionId)），复用
    // 📷 按钮同一条 captureTerminalToDataUrl + saveScreenshot 管线，产出
    // 与手动截图逐字节一致 —— 不绕 sessionActions 是因为那条注册表是
    // fire-and-forget 的，拿不到保存路径，而路径是这次改动的核心交付。
    //
    // 不抢焦点：截的是该连接标签页的缓冲区内容，与用户当前在看哪个标签
    // 无关；exec_in_tab 曾因无条件 set_focus 被用户投诉，截图这条路径
    // 更不该重蹈。已有标签时直接复用，没有才开新标签（新标签会置为活动
    // 态，这是「打开标签」语义自带的，无法避免也不需要避免）。
    if (action === "screenshot_terminal" && event.payload.request_id) {
      const requestId = event.payload.request_id;
      // 调用方可选的文件名标签（P1：跳转采集时表达「这是哪台目标机的画面」）。
      // 连接身份仍会进入文件名 —— 丢了它就答不出「这张图本来是在哪台上截的」。
      const shotLabel = typeof event.payload.label === "string" ? event.payload.label : undefined;
      const fail = (error: string) => mcpExecResult(requestId, { ok: false, error });

      const captureTab = (tabId: string) => {
        const tab = getTab(tabId);
        if (!tab?.sessionId || tab.status !== "connected") {
          fail("该连接的终端标签页尚未连接，无法截图。请先在 GUI 中打开并连上该连接。");
          return;
        }
        const term = getTerminal(tab.sessionId);
        if (!term) {
          fail("终端实例未就绪（面板可能仍在挂载中），请稍候重试。");
          return;
        }
        // 与 TerminalPanel.handleScreenshot 同管线；保存路径在这里回传给
        // MCP 调用方，toast 也照旧给用户一个可「打开」的成功反馈。
        void (async () => {
          try {
            const dataUrl = await captureTerminalToDataUrl(term);
            if (!dataUrl) {
              fail("截图失败：无法捕获终端画面");
              return;
            }
            const path = await saveScreenshot(
              dataUrl,
              tab.name || tab.sessionId!,
              shotLabel
            );
            toast(`已保存：${path}`, {
              type: "success",
              duration: 8000,
              action: { label: "打开", onClick: () => showInFolder(path) },
            });
            mcpExecResult(requestId, { ok: true, path });
          } catch (e: unknown) {
            const msg = e instanceof Error && e.message ? String(e.message) : String(e);
            fail(msg.includes("附件目录") ? msg : `保存失败: ${msg}`);
          }
        })();
      };

      const existing = sessions.tabs.find(
        (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "connected",
      );
      if (existing?.sessionId) {
        captureTab(existing.id);
        return;
      }

      // 没有已连接的终端标签页：开一个新标签并等它连上后截图。
      // 复用 exec_in_tab 的新建 tab 路径语义（快照 id 识别本次新建的那个）。
      const idsBefore = new Set(sessions.tabs.map((t) => t.id));
      connect(config);
      let attempts = 0;
      // 30s 轮询上限（60 × 500ms），与三端超时预算对齐：GUI 侧 IPC 等 45s、
      // MCP 侧读 50s，都比这里大 —— 宁可 GUI 先超时回一条结构化错误，
      // 也不要前端还没连上 GUI 就已经答完了（那会变成假成功）。
      const maxAttempts = 60;
      const checkInterval = setInterval(() => {
        attempts++;
        const tab = sessions.tabs.find(
          (t) => !idsBefore.has(t.id) && t.connectionId === connection_id && t.type === "terminal",
        );
        if (tab?.status === "error") {
          clearInterval(checkInterval);
          activeScreenshotPolls.delete(checkInterval);
          fail(`连接失败：${tab.errorMessage || "请检查保险库是否已解锁"}`);
          return;
        }
        if (tab?.sessionId && tab.status === "connected") {
          clearInterval(checkInterval);
          activeScreenshotPolls.delete(checkInterval);
          captureTab(tab.id);
        } else if (attempts >= maxAttempts) {
          clearInterval(checkInterval);
          activeScreenshotPolls.delete(checkInterval);
          fail("连接超时，无法建立终端会话，截图未发起。");
        }
      }, 500);
      activeScreenshotPolls.add(checkInterval);
      return;
    }

    // ── exec_in_tab：在可见终端 tab 里执行命令并回传输出 ──
    if (action === "exec_in_tab" && event.payload.request_id && event.payload.command) {
      const requestId = event.payload.request_id;
      const command = event.payload.command;
      const timeoutMs = (event.payload.timeout || 30) * 1000;
      // 进程级隔离键：连接 × MCP 客户端进程。pid 缺失（旧版 MCP 二进制）时
      // 落到 ":0"，与历史“共享单标签”行为兼容 —— 新版 MCP 一起升级即隔离。
      const processKey = `${connection_id}:${event.payload.client_pid ?? 0}`;

      // 互斥锁检查 + 陈旧锁自愈（阈值随本请求 timeout 放大）
      const lockStaleMs = Math.max(MCP_LOCK_STALE_MS, timeoutMs + 10_000);
      const existingLockAt = mcpExecLocks.get(processKey);
      if (existingLockAt !== undefined) {
        const lockAge = Date.now() - existingLockAt;
        if (lockAge > lockStaleMs) {
          console.warn(`[mcp-exec] stale lock on ${processKey} (${lockAge}ms old), force-releasing`);
          mcpExecLocks.delete(processKey);
        } else {
          const remainSec = Math.ceil((lockStaleMs - lockAge) / 1000);
          mcpExecResult(requestId, {
            ok: false,
            error:
              `上一条命令仍在本进程该服务器的终端中执行（已运行约 ${Math.round(lockAge / 1000)} 秒）。` +
              "交互式终端一次只能跑一条命令，请等待上一条结束后重试" +
              (remainSec > 0 ? `（预计最多再等 ${remainSec} 秒）` : "") +
              "。命令未发送到远端，可以安全重试。" +
              "后续如需运行耗时命令，请改用 `ssh_run`（异步，不受此锁限制，可用 `ssh_cancel` 取消），" +
              "或用 nohup 后台执行（如 `nohup pip install -e . > /tmp/log 2>&1 &`）后轮询日志文件。",
          });
          return;
        }
      }
      mcpExecLocks.set(processKey, Date.now());
      // 会话级授权标记：用户在本次确认框点了「本轮会话均允许」时为 true，
      // 随执行结果回传 MCP（其进程内记住，后续所有确认点直接放行）。
      let sessionAllowedGranted = false;
      // MCP 侧传入的会话授权（Rust 进程内标志）。GUI 是权威判定方：用户在顶栏
      // 撤销后（mcpSessionAllowed=false），此字段被忽略并回带 session_revoked。
      const mcpClaimsSessionAllowed = event.payload.session_allowed === true;
      const sessionAllowedByMcp = mcpClaimsSessionAllowed && ui.mcpSessionAllowed;
      // 撤销信号：MCP 声称已授权但 GUI 已撤销 → 让 MCP 清除自己的标志
      const sessionRevoked = mcpClaimsSessionAllowed && !ui.mcpSessionAllowed;

      // 所有完成路径都要释放锁 + 回传结果。
      // command_sent 必须如实反映「命令是否已经写进 PTY」——MCP 侧据此决定
      // 能不能无头重跑：已经发出去的命令再跑一次，会在用户看不见的地方执行两遍。
      let commandSent = false;
      let settled = false;
      // 兜底看门狗句柄：在 finishExec 之前声明，任何完成路径都能取消它。
      let watchdog: number | undefined;
      // 输出订阅的解绑钩子，由 runExec 在挂好监听后装入。finishExec 统一调用它，
      // 保证「看门狗超时」「硬超时」「正常收尾」三条路径都不会漏掉 IPC 监听。
      let stopOutput: (() => void) | null = null;
      // exit_code 允许 null：空闲回退时命令「是否跑完」未确认，如实上报 null
      // 而不是默认 0。completed 缺省视为 true（快命令的正常路径不带该字段）。
      const finishExec = (result: { ok: boolean; stdout?: string; exit_code?: number | null; completed?: boolean; error?: string }) => {
        if (settled) return;
        settled = true;
        clearTimeout(watchdog);
        stopOutput?.();
        mcpExecLocks.delete(processKey);
        const payload: Record<string, unknown> = { ...result, command_sent: commandSent };
        if (sessionAllowedGranted) payload.session_allowed = true;
        if (sessionRevoked) payload.session_revoked = true;
        mcpExecResult(requestId, payload);
      };

      // 兜底看门狗：从拿到锁的这一刻起计时，覆盖确认弹窗、重连/建连、命令执行
      // 全链路。Rust 侧从 IPC 发出后就等待 timeout+10s，超时只会回一个信息量
      // 很低的 `exec timeout (Xs)`；这里把预算设在它之前（timeout+9s），保证 AI
      // 拿到的是一条可行动的中文原因。同时任何未预见的挂起（对话框无人点击、
      // sshSend 抛错、sentinel 丢失）都必然经由 finishExec 释放锁 —— 旧实现里
      // 这类挂起会让该连接一直锁到前端下一次超时，期间所有命令都被拒绝。
      watchdog = setTimeout(() => {
        finishExec({
          ok: false,
          error:
            `命令在本机 GUI 侧未能在 ${Math.round((timeoutMs + 9_000) / 1000)} 秒内完成` +
            `（可能原因：确认弹窗未处理、会话重连失败或终端长时间无输出）。` +
            `命令是否已执行：${commandSent ? "已发送到远端，可能仍在运行" : "尚未发送"}。` +
            "请重试，或改用 nohup 后台执行 + 轮询日志。",
        });
      }, timeoutMs + 9_000);

      // 进程级标签解析：优先用注册表里本进程自己的标签（且仍存活）；
      // 旧版 MCP（无 client_pid → pid=0）回落到历史行为「任一同连接终端标签」，
      // 保证新旧二进制共存期间不炸。
      const registeredId = mcpProcessTabs.get(processKey);
      let existingTab = registeredId
        ? sessions.tabs.find((t) => t.id === registeredId && t.connectionId === connection_id && t.type === "terminal")
        : undefined;
      if (!existingTab && !event.payload.client_pid) {
        existingTab = sessions.tabs.find((t) => t.connectionId === connection_id && t.type === "terminal");
      }
      if (existingTab) mcpProcessTabs.set(processKey, existingTab.id);

      const runExec = async (sessionId: string) => {
        // ── GUI 侧命令确认（黑名单命中时）──
        // fail-closed：show_in_gui 模式下这个对话框是唯一的确认门（服务端的
        // command_rules 分类在这条路径上根本不会被求值），因此规则加载或解析
        // 失败时必须照常弹窗，绝不能因为一次 IPC/IO 错误就放行。
        if (!sessionAllowedGranted && !sessionAllowedByMcp) {
          let needsConfirm = true;
          let policyError: string | null = null;
          try {
            needsConfirm = await checkCommandConfirmation(command);
          } catch (e) {
            policyError = e instanceof Error ? e.message : String(e);
            console.warn("[mcp-gui] 命令规则校验失败，按需确认处理:", e);
          }

          if (needsConfirm) {
            const connectionName = config.name || connection_id;
            const reasons = await checkCommandDangerReasons(command).catch(() => [] as string[]);
            if (policyError) {
              reasons.unshift(`⚠ 无法校验命令规则（${policyError}），已按需确认处理`);
            }
            const decision = await showMcpConfirm(command, connectionName, reasons);
            if (decision === "session") {
              // 本轮会话均允许：标记本次结果带 session_allowed，并立即执行本条命令。
              // MCP 服务器收到该标记后在自身进程内记住授权（进程生命周期 = 一轮
              // AI 会话），后续所有高危命令跳过确认直接执行。
              sessionAllowedGranted = true;
            } else if (decision === "deny") {
              finishExec({
                ok: false,
                // 与 myshell-mcp.rs 的 denied_by_user_text() 保持同步——
                // 拒绝即硬性停止：AI 必须停下全部操作等待用户决定。
                error:
                  `⛔ 用户已拒绝高危操作：ssh_exec（在服务器 [${connectionName}] 执行命令: ${command}）。\n\n` +
                  "【硬性停止】请立即停止当前任务的所有后续操作：不要重试该命令，" +
                  "不要改用其他工具、路径或连接绕过，也不要继续任务中的下一步。\n" +
                  "请立即做两件事：\n" +
                  "1. 向用户输出当前任务说明：你正在执行什么任务、已完成哪些步骤、" +
                  "被拒绝的是哪一步、剩余计划是什么；\n" +
                  "2. 停止并等待用户明确答复。只有用户明确表示继续后才可以恢复操作，" +
                  "用户答复前不要发起任何 MCP 调用。",
              });
              return;
            }
          }
        }

        // sentinel 机制：命令后跟 `echo __MCP_DONE_<uuid>__:$?`，
        // 从输出流里截取命令回显与 sentinel 结果行之间的部分作为 stdout。
        const sentinel = `__MCP_DONE_${Math.random().toString(36).slice(2, 14)}__`;

        let outputBuf = "";
        let done = false;
        let timedOut = false;
        let lastDataAt = Date.now();
        let sawSentinelEcho = false;
        // 流式解码器：PTY 输出按 16ms/16KiB 批量 flush，一个多字节 UTF-8 字符
        // （中文）恰好被切成两片时，无状态 decode 会把前半段变成不可恢复的
        // U+FFFD。带 stream:true 的单实例解码器跨 chunk 保留那半个字符。
        const decoder = new TextDecoder("utf-8", { fatal: false });

        // 在原始字节流里跳过 ANSI/OSC 转义与 \r 之后找 sentinel 结果行，
        // 返回 [buf 中的原始下标, 退出码]；找不到返回 null。
        //
        // 折行时 readline 会在行中间插入光标移动转义（CSI），且这些转义恰好
        // 可能落在 sentinel 内部 —— 所以在原始串上 startsWith 会脱靶，而那
        // 正是「含管道/重定向的命令固定多等 5-6 秒」的根因。做法：先构造
        // 「可见字符 → 原始下标」的映射，再在可见文本上匹配并取码，最后把
        // 下标翻译回原始位置（截断输出要用）。
        const findSentinelResult = (buf: string, from: number): [number, number] | null => {
          // 一遍扫描：剥掉转义，同时记录每个可见字符的原始下标。
          let visible = "";
          const map: number[] = [];
          let i = from;
          while (i < buf.length) {
            const ch = buf[i];
            if (ch === "\x1b") {
              if (buf[i + 1] === "[") {
                i += 2;
                while (i < buf.length && !/[a-zA-Z]/.test(buf[i])) i += 1;
                i += 1;
                continue;
              }
              if (buf[i + 1] === "]") {
                i += 2;
                while (i < buf.length && buf[i] !== "\x07" && buf[i] !== "\x1b") i += 1;
                i += buf[i] === "\x07" ? 1 : 2;
                continue;
              }
              i += 2;
              continue;
            }
            if (ch === "\r") {
              i += 1;
              continue;
            }
            visible += ch;
            map.push(i);
            i += 1;
          }
          // 二遍：在可见文本上找结果行。只认 `:<数字>`：命令回显里的是 `:$?`，
          // `$` 不是数字，借此把「用户输入的那一行」和「echo 真正打出的结果」
          // 区分开。
          const vIdx = visible.indexOf(sentinel);
          if (vIdx < 0) return null;
          const after = visible.slice(vIdx + sentinel.length);
          const codeMatch = /^:(\d+)/.exec(after);
          if (!codeMatch) return null;
          return [map[vIdx], parseInt(codeMatch[1], 10)];
        };

        // 去掉 ANSI/OSC 与 \r 的可见文本，仅用于提示符形态判断。
        // 续行（PS2）检测：shell 因上一条命令引号/heredoc 未闭合而停在 `>`
        // 时，我们注入的命令与 sentinel 都会被当作续行文本回显，每行带 `> `
        // 前缀且永不执行。正常执行时该行前缀是 PS1（`user@host:~$ `），`>`
        // 不在行首 —— 这就是判别式。命中即补 Ctrl+C 清场并快速失败，而不是
        // 让后续每条命令都静默失效（反馈 P0-2）。
        const detectContinuation = (buf: string): boolean =>
          /(?:^|\n)>\s*echo __MCP_DONE_/.test(
            buf
              .replace(/\x1b\][\s\S]*?(?:\x07|\x1b\\)/g, "")
              .replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "")
              .replace(/\r/g, "")
              .slice(-4096),
          );

        // 输出收尾：截 sentinel、归一化换行、剥命令回显与 helper 行（保留 ANSI 颜色）
        // completed=false 表示「没能确认命令跑完」（空闲回退/超时兜底），此时
        // exit_code 必须原样上报 null，不能默认成 0 —— 那会把未完成当成成功。
        const finalizeOutput = (exitCode: number | null, completed: boolean) => {
          done = true;
          let stdout = outputBuf;
          const sentinelRe = new RegExp(sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)");
          const sentinelMatch = stdout.match(sentinelRe);
          if (sentinelMatch && sentinelMatch.index !== undefined) {
            stdout = stdout.slice(0, sentinelMatch.index);
          }

          stdout = stdout.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
          stdout = stdout.replace(/\n+$/, "");
          const lastNl = stdout.lastIndexOf("\n");
          const lastLine = stdout.slice(lastNl + 1);
          if (lastLine && /[#$]\s*$/.test(lastLine)) {
            stdout = stdout.slice(0, lastNl);
          }

          // 剥命令回显：定位命令文本本身，取其后整行之后的内容
          let start = stdout.indexOf(command);
          if (start >= 0) {
            start += command.length;
            const lineEnd = stdout.indexOf("\n", start);
            stdout = lineEnd >= 0 ? stdout.slice(lineEnd + 1) : "";
          } else {
            const lines = stdout.split("\n");
            if (lines.length > 0 && command.startsWith(lines[0].trim())) {
              lines.shift();
            }
            stdout = lines.join("\n");
          }

          // 剥 sentinel helper 行回显：用去 ANSI 副本定位，再按可见位置回原串切割
          const ansiCleaned = stdout.replace(/\x1b\]\d+;.*?(?:\x07|\x1b\\)/g, "").replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
          const helperIdx = ansiCleaned.search(/echo __MCP_DONE_/);
          if (helperIdx >= 0) {
            const lineStart = ansiCleaned.lastIndexOf("\n", helperIdx);
            const cutVisible = lineStart >= 0 ? lineStart : helperIdx;
            stdout = stripFromAnsiPosition(stdout, cutVisible);
          }
          // 刻意不清 ANSI：AI 看到的颜色与终端一致
          stdout = stdout.replace(/^\n+/, "").replace(/\n+$/, "");

          finishExec({
            ok: true,
            stdout,
            // 未能确认完成时如实上报 null + completed=false：把「没等到结果」
            // 静默转成 0 会让调用方把未执行的命令当成成功。
            exit_code: completed ? (exitCode ?? 0) : null,
            completed,
          });
        };

        const unlistenOutput = await onSshOutput(sessionId, (data) => {
          if (done) return;
          lastDataAt = Date.now();
          outputBuf += decoder.decode(data, { stream: true });

          const MAX_OUTPUT = 4 * 1024 * 1024;
          if (outputBuf.length > MAX_OUTPUT) {
            outputBuf = outputBuf.slice(-MAX_OUTPUT);
          }

          if (!sawSentinelEcho && outputBuf.includes(`echo ${sentinel}`)) {
            sawSentinelEcho = true;
          }

          // 续行自愈：一旦看到 sentinel 回显带 `> ` 前缀，说明 shell 仍停在
          // PS2（上一条命令引号未闭合）。此时发 Ctrl+C 清场并直接失败——
          // 继续等只会让每条命令都静默失效（反馈 P0-2：连续 7 次全部进 `>`）。
          if (!done && detectContinuation(outputBuf)) {
            sshSend(sessionId, "\x03").catch(() => {});
            finishExec({
              ok: false,
              error:
                "上一命令的引号/heredoc 未闭合，shell 仍处于续行（>）状态，本条命令未被执行。" +
                "已自动发送 Ctrl+C 恢复提示符；请重试，并检查前一条命令是否缺少闭合引号。",
            });
            return;
          }

          // 折行容错匹配：回显被 readline 折行时会插入光标移动转义，
          // 直接在原始串上匹配 sentinel 结果行会脱靶，进而触发 5 秒空闲
          // 回退 —— 那正是「含 | / 2>&1 / ; 的命令固定 6 秒」的来源。
          const found = findSentinelResult(outputBuf, Math.max(0, outputBuf.length - 64 * 1024));
          if (!found) return;
          const [matchIndex, exitCode] = found;
          outputBuf = outputBuf.slice(0, matchIndex);
          finalizeOutput(exitCode, true);
        });
        // 监听已挂好 —— 交给 finishExec 统一解绑（看门狗/硬超时/正常收尾都走它）。
        stopOutput = unlistenOutput;

        // 硬超时计时器必须先于「发命令」注册：sshSend 自身失败时旧代码会直接
        // 抛出，两条 setInterval 与硬超时全都还没挂上——前端从此对这条请求永久
        // 沉默，Rust 只能干等到 timeout+10s 回一个 `exec timeout`。现在无论
        // sshSend 成功还是抛错，都有计时器兜住并回结果。
        //
        // 三层完成策略：sentinel 结果行 → 空闲回退（5s 无新数据）→ 硬超时
        const IDLE_TIMEOUT_MS = 5000;
        let idleCheck: number | undefined;
        setTimeout(() => {
          if (!done && !timedOut) {
            timedOut = true;
            clearInterval(idleCheck);
            // 先补 Ctrl+C 清场再报错，让下一条命令拿到干净提示符
            sshSend(sessionId, "\x03").catch(() => {});
            finishExec({
              ok: false,
              error:
                `命令超时（${event.payload.timeout || 30}秒未完成）。已自动发送 Ctrl+C 中断残留进程；` +
                "如需长时间运行的命令，请用 nohup 后台执行并轮询日志。",
              stdout: outputBuf,
            });
          }
        }, timeoutMs);

        try {
          // TERM 显式声明（P0-3）：gui 通道的 PTY 由 ssh.rs 以
          // `xterm-256color` 申请，但 TERM 变量是否真的到远端取决于 sshd 的
          // AcceptEnv —— 没声明时远端 shell 报
          // `TERM environment variable not set.`，`clear` 之类依赖 terminfo
          // 的命令行为不确定（这正是「像终端又不是终端」的中间态）。
          //
          // 每会话只探针一次：探针本身是一次往返，且它的输出会进入
          // outputBuf —— 必须先于 sentinel 订阅之前发完并等待其回显归位，
          // 否则探针输出会被算进命令 stdout。做法：探针在订阅输出前发送，
          // 用一个独立的一次性监听只吞这一行。
          await ensureTermDeclared(sessionId);
          // 监听挂好后再发命令（先订阅后发送，避免快命令输出丢失）
          await sshSend(sessionId, command + "\n");
          // 从这一刻起命令已经在远端跑了——后续任何失败（硬超时、sentinel 丢失、
          // IPC 断开）都必须在结果里如实标记，MCP 才知道不能无头重跑。
          commandSent = true;
          await new Promise((r) => setTimeout(r, 80));
          await sshSend(sessionId, `echo ${sentinel}:$?\n`);
        } catch (e) {
          finishExec({ ok: false, error: `命令发送失败: ${e}` });
          return;
        }

        idleCheck = setInterval(() => {
          if (done || timedOut) {
            clearInterval(idleCheck);
            return;
          }
          if (sawSentinelEcho && Date.now() - lastDataAt >= IDLE_TIMEOUT_MS) {
            clearInterval(idleCheck);
            // completed=false：这是兜底收场，不是确认完成。旧代码在这里报
            // exit_code 0，让「sentinel 脱靶」伪装成命令成功。
            finalizeOutput(null, false);
          }
        }, 1000);

      };

      if (existingTab && existingTab.status === "connected" && existingTab.sessionId) {
        // 快路径：tab 已连接，直接执行
        focusTabIfIdle(existingTab.id);
        runExec(existingTab.sessionId).catch((e) => {
          finishExec({ ok: false, error: `执行失败: ${e}` });
        });
      } else if (existingTab && (existingTab.status === "disconnected" || existingTab.status === "error")) {
        // 重连路径：原地重连（保留 scrollback）后执行
        focusTabIfIdle(existingTab.id);
        reconnectOne(existingTab.id).then(async (ok) => {
          if (!ok) {
            finishExec({
              ok: false,
              error: `会话已断开且重连失败：${existingTab.errorMessage || "请检查网络和保险库是否解锁"}`,
            });
            return;
          }
          const reconnected = getTab(existingTab.id);
          if (reconnected?.sessionId && reconnected.status === "connected") {
            runExec(reconnected.sessionId).catch((e) => {
              finishExec({ ok: false, error: `执行失败: ${e}` });
            });
          } else {
            finishExec({ ok: false, error: "重连后未找到会话" });
          }
        }).catch((e) => {
          finishExec({ ok: false, error: `重连失败: ${e}` });
        });
      } else {
        // 新 tab 路径：开 tab + 连接，轮询等**本次新建**的标签连上后执行。
        // 不能再按 connectionId 找“任一已连接终端标签”——本进程的新标签还在
        // 握手时，其它进程的旧标签可能已连接，那样会登错标签、隔离失效。
        // 用「调用前已有标签 id 快照」识别新增的那一个。
        const idsBefore = new Set(sessions.tabs.map((t) => t.id));
        connect(config);
        let attempts = 0;
        const maxAttempts = Math.min(Math.floor(timeoutMs / 500), 60);
        const checkInterval = setInterval(() => {
          attempts++;
          const tab = sessions.tabs.find(
            (t) => !idsBefore.has(t.id) && t.connectionId === connection_id && t.type === "terminal",
          );
          if (tab && tab.status === "error") {
            clearInterval(checkInterval);
            finishExec({
              ok: false,
              error: `连接失败：${tab.errorMessage || "请检查保险库是否已解锁"}`,
            });
            return;
          }
          if (tab && tab.sessionId && tab.status === "connected") {
            clearInterval(checkInterval);
            mcpProcessTabs.set(processKey, tab.id);
            focusTabIfIdle(tab.id);
            runExec(tab.sessionId).catch((e) => {
              finishExec({ ok: false, error: `执行失败: ${e}` });
            });
          } else if (attempts >= maxAttempts) {
            clearInterval(checkInterval);
            finishExec({ ok: false, error: "连接超时，无法建立终端会话" });
          }
        }, 500);
      }
      return;
    }

    // ── open_connection：打开/聚焦 tab（fire-and-forget）──
    if (action !== "open_connection") return;

    const tab_type = event.payload.tab_type || "auto";
    const focus_existing = event.payload.focus_existing ?? true;

    const wantType: "terminal" | "sftp" | "auto" = tab_type === "sftp" ? "sftp" : tab_type === "terminal" ? "terminal" : "auto";

    if (focus_existing) {
      const existing = sessions.tabs.find((t) => {
        if (t.connectionId !== connection_id) return false;
        if (wantType === "sftp") return t.type === "sftp";
        if (wantType === "terminal") return t.type === "terminal";
        return true;
      });
      if (existing) {
        // open_connection 的语义就是「打开并聚焦」——这里保留切页，它是
        // MCP 客户端显式请求的（如 open_in_gui 工具），不是每次 exec 的副作用。
        sessions.activeTabId = existing.id;
        return;
      }
    }

    if (wantType === "sftp" && (config.conn_type === "ssh" || config.conn_type === "sftp" || !config.conn_type)) {
      connect({ ...config, conn_type: "sftp" });
    } else {
      connect(config);
    }
  });
  // 注册表清理：用户关掉某进程的终端标签后，其注册项随之失效，下次 exec
  // 会为该进程开新标签（而不是复用已死的 tabId）。
  const stopWatch = watch(
    () => sessions.tabs.map((t) => t.id).join(","),
    () => {
      const live = new Set(sessions.tabs.map((t) => t.id));
      for (const [key, tabId] of mcpProcessTabs) {
        if (!live.has(tabId)) mcpProcessTabs.delete(key);
      }
    },
  );
  return () => {
    unlisten();
    stopWatch();
    // 截图轮询的卸载清理：桥断开时还有轮询在跑（前端正在等新标签连上），
    // 这里必须停 —— 否则 interval 会带着 sessions.tabs 引用转到应用结束。
    for (const handle of activeScreenshotPolls) clearInterval(handle);
    activeScreenshotPolls.clear();
  };
}
