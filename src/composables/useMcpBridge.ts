// MCP ↔ GUI 桥：监听 MCP 服务器的 open_connection / exec_in_tab 命令。
// 从旧 App.tsx 的 mcp-gui-command useEffect 原样移植（sentinel 三层完成策略、
// 每连接执行互斥锁 + 陈旧锁自愈、GUI 确认对话框），语义零改动。
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  checkCommandConfirmation,
  checkCommandDangerReasons,
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
}

// 每连接执行锁：连接 id → 加锁时刻。交互式 PTY 串行执行，上一条没跑完时
// 拒绝注入下一条（避免命令字节交错 / sentinel 串台）。超过 MCP_LOCK_STALE_MS
// 的陈旧锁强制释放（自愈，见旧 App.tsx 注释）。
const mcpExecLocks = new Map<string, number>();
const MCP_LOCK_STALE_MS = 60_000;

export async function startMcpBridge(): Promise<UnlistenFn> {
  return listen<McpGuiCommand>("mcp-gui-command", (event) => {
    const { action, connection_id } = event.payload;
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

      // 互斥锁检查 + 陈旧锁自愈
      const existingLockAt = mcpExecLocks.get(connection_id);
      if (existingLockAt !== undefined) {
        const lockAge = Date.now() - existingLockAt;
        if (lockAge > MCP_LOCK_STALE_MS) {
          console.warn(`[mcp-exec] stale lock on ${connection_id} (${lockAge}ms old), force-releasing`);
          mcpExecLocks.delete(connection_id);
        } else {
          mcpExecResult(requestId, {
            ok: false,
            error:
              "上一条命令仍在该服务器的终端中执行。交互式终端一次只能跑一条命令。" +
              "如需运行耗时命令，请用 nohup 后台执行（如 `nohup pip install -e . > /tmp/log 2>&1 &`），" +
              "然后轮询日志文件查看结果。",
          });
          return;
        }
      }
      mcpExecLocks.set(connection_id, Date.now());

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
      const finishExec = (result: { ok: boolean; stdout?: string; exit_code?: number; error?: string }) => {
        mcpExecLocks.delete(connection_id);
        const payload: Record<string, unknown> = { ...result, command_sent: commandSent };
        if (sessionAllowedGranted) payload.session_allowed = true;
        if (sessionRevoked) payload.session_revoked = true;
        mcpExecResult(requestId, payload);
      };

      const existingTab = sessions.tabs.find((t) => t.connectionId === connection_id && t.type === "terminal");

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

        // 输出收尾：截 sentinel、归一化换行、剥命令回显与 helper 行（保留 ANSI 颜色）
        const finalizeOutput = (exitCode: number | null) => {
          done = true;
          unlistenOutput();
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

          finishExec({ ok: true, stdout, exit_code: exitCode ?? 0 });
        };

        const unlistenOutput = await onSshOutput(sessionId, (data) => {
          if (done) return;
          lastDataAt = Date.now();
          outputBuf += new TextDecoder("utf-8", { fatal: false }).decode(data);

          const MAX_OUTPUT = 4 * 1024 * 1024;
          if (outputBuf.length > MAX_OUTPUT) {
            outputBuf = outputBuf.slice(-MAX_OUTPUT);
          }

          if (!sawSentinelEcho && outputBuf.includes(`echo ${sentinel}`)) {
            sawSentinelEcho = true;
          }

          const sentinelRe = new RegExp(sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)");
          const tailSearch = outputBuf.slice(-16 * 1024);
          const tailMatch = tailSearch.match(sentinelRe);
          if (!tailMatch) return;
          const matchIndex = outputBuf.length - tailSearch.length + (tailMatch.index ?? 0);
          const exitCode = parseInt(tailMatch[1], 10);
          outputBuf = outputBuf.slice(0, matchIndex);
          finalizeOutput(exitCode);
        });

        // 监听挂好后再发命令（先订阅后发送，避免快命令输出丢失）
        await sshSend(sessionId, command + "\n");
        // 从这一刻起命令已经在远端跑了——后续任何失败（硬超时、sentinel 丢失、
        // IPC 断开）都必须在结果里如实标记，MCP 才知道不能无头重跑。
        commandSent = true;
        await new Promise((r) => setTimeout(r, 80));
        await sshSend(sessionId, `echo ${sentinel}:$?\n`);

        // 三层完成策略：sentinel 结果行 → 空闲回退（5s 无新数据）→ 硬超时
        const IDLE_TIMEOUT_MS = 5000;
        const idleCheck = setInterval(() => {
          if (done || timedOut) {
            clearInterval(idleCheck);
            return;
          }
          if (sawSentinelEcho && Date.now() - lastDataAt >= IDLE_TIMEOUT_MS) {
            clearInterval(idleCheck);
            finalizeOutput(null);
          }
        }, 1000);

        setTimeout(() => {
          if (!done && !timedOut) {
            timedOut = true;
            clearInterval(idleCheck);
            unlistenOutput();
            // 先补 Ctrl+C 清场再报错，让下一条命令拿到干净提示符
            sshSend(sessionId, "\x03").catch(() => {});
            finishExec({
              ok: false,
              error: `命令超时（${event.payload.timeout || 30}秒未完成）。已自动发送 Ctrl+C 中断残留进程；如需长时间运行的命令，请用 nohup 后台执行并轮询日志。`,
              stdout: outputBuf,
            });
          }
        }, timeoutMs);

        const cleanupCheck = setInterval(() => {
          if (done || timedOut) {
            clearInterval(cleanupCheck);
            unlistenOutput();
          }
        }, 500);
      };

      if (existingTab && existingTab.status === "connected" && existingTab.sessionId) {
        // 快路径：tab 已连接，直接执行
        sessions.activeTabId = existingTab.id;
        runExec(existingTab.sessionId).catch((e) => {
          finishExec({ ok: false, error: `执行失败: ${e}` });
        });
      } else if (existingTab && (existingTab.status === "disconnected" || existingTab.status === "error")) {
        // 重连路径：原地重连（保留 scrollback）后执行
        sessions.activeTabId = existingTab.id;
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
        });
      } else {
        // 新 tab 路径：开 tab + 连接，轮询等连上后执行
        connect(config);
        let attempts = 0;
        const maxAttempts = Math.floor(timeoutMs / 500);
        const checkInterval = setInterval(() => {
          attempts++;
          const errTab = sessions.tabs.find(
            (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "error",
          );
          if (errTab) {
            clearInterval(checkInterval);
            finishExec({
              ok: false,
              error: `连接失败：${errTab.errorMessage || "请检查保险库是否已解锁"}`,
            });
            return;
          }
          const tab = sessions.tabs.find(
            (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "connected" && t.sessionId,
          );
          if (tab && tab.sessionId) {
            clearInterval(checkInterval);
            sessions.activeTabId = tab.id;
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
}
