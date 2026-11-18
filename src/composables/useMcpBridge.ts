// MCP ↔ GUI 桥：监听 MCP 服务器的 open_connection / exec_in_tab 命令。
// 从旧 App.tsx 的 mcp-gui-command useEffect 原样移植（sentinel 三层完成策略、
// 每连接执行互斥锁 + 陈旧锁自愈、GUI 确认对话框），语义零改动。
import { watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  checkCommandConfirmation,
  checkCommandDangerReasons,
  mcpConfirmResult,
  mcpExecResult,
  onSshOutput,
  sshSend,
} from "../api";
import { connectionsStore } from "../store/connections";
import { sessions, connect, reconnectOne, getTab } from "../store/sessions";
import { showMcpConfirm, ui } from "../store/ui";
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
}

// 每连接执行锁：连接 id → 加锁时刻。交互式 PTY 串行执行，上一条没跑完时
// 拒绝注入下一条（避免命令字节交错 / sentinel 串台）。
//
// 陈旧锁自愈阈值不是固定值：MCP 侧允许 AI 传 timeout（默认 60s，最大 3600s），
// 固定 60s 会在长命令仍在跑时把锁强释，第二条命令随即注入同一条 PTY —— 正是
// 锁要防的 sentinel 串台。阈值 = max(MCP_LOCK_STALE_MS, 本请求 timeout + 10s)。
const mcpExecLocks = new Map<string, number>();
const MCP_LOCK_STALE_MS = 60_000;

// 进程级终端标签注册表："连接id:客户端pid" → tabId。exec_in_tab 据此复用
// 本进程自己的标签；标签被用户关掉后从这里清除（startMcpBridge 内 watch）。
// MCP 进程重启换 pid → 自然落新键、开新标签，旧标签留作孤儿由用户手关
//（断开≠退出，不自动关）。
const mcpProcessTabs = new Map<string, string>();

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

    const config = connectionsStore.connections.find((c) => c.id === connection_id);
    if (!config) {
      console.warn(`[mcp-gui] connection not found: ${connection_id}`);
      if (action === "exec_in_tab" && event.payload.request_id) {
        mcpExecResult(event.payload.request_id, { ok: false, error: "未找到连接" });
      }
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
  };
}
