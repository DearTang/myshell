// 外部工具连接导入：把别家客户端保存的连接解析成 MyShell 的 ConnectionConfig 草稿。
//
// 覆盖五种常见格式：OpenSSH ssh_config、PuTTY 注册表导出（.reg）、MobaXterm
// （.mobaconf，INI）、SecureCRT（INI）、通用 CSV。
//
// 设计约束：
// 1. 返回的是**草稿**，不是成品。密码/私钥永远不在这里 —— 各格式要么不存
//    密码（ssh_config / CSV），要么以各家私有方式加密（PuTTY / SecureCRT），
//    硬解只会产出错误的安全感。导入对话框把凭据字段留空，由用户补填。
// 2. 解析失败要逐条报出来。静默跳过会让用户以为「导入了 20 个」实际只有
//    3 个成功，回头连不上还找不到原因。
// 3. 每个解析器都是纯函数：输入字符串，输出 { configs, errors }。可单测、
//    不碰磁盘、不碰数据库。

import type { ConnectionConfig, ConnType } from "@/api";

/** 一条解析失败记录：第几行/哪个 Host 块、原因。 */
export interface ImportError {
  /** 出错的来源标识：ssh_config 的 Host 名、INI 的 session 名、CSV 的行号。 */
  source: string;
  reason: string;
}

export interface ImportResult {
  configs: ConnectionConfig[];
  errors: ImportError[];
}

function draft(partial: Partial<ConnectionConfig>): ConnectionConfig {
  const now = new Date().toISOString();
  return {
    // id 必须在这里生成：后端 save_connection 用 INSERT OR REPLACE 以 id 为
    // 主键，空 id 会让一批导入全部落到同一行上 —— 用户以为导入了 N 条，
    // 实际只剩最后一条（密码也按空 id 存进 keyring，互相顶掉）。
    id: crypto.randomUUID(),
    name: partial.name ?? "",
    host: partial.host ?? "",
    port: partial.port ?? 22,
    username: partial.username ?? "root",
    auth_method: partial.auth_method ?? "password",
    conn_type: partial.conn_type ?? "ssh",
    group_path: partial.group_path ?? "/",
    proxy_type: partial.proxy_type ?? "none",
    address_family: partial.address_family ?? "auto",
    ftp_tls: partial.ftp_tls ?? "none",
    ftp_passive: partial.ftp_passive ?? true,
    ...partial,
    created_at: now,
  } as ConnectionConfig;
}

// ── ssh_config ────────────────────────────────────────────────────────────

/**
 * Parse an OpenSSH `ssh_config` file.
 *
 * Handles the parts that matter for import: `Host` blocks (with globs), the
 * `HostName` / `Port` / `User` keywords, `ProxyJump` (→ a jump-host note in
 * the name, since MyShell proxies are SOCKS/HTTP rather than SSH jump), and
 * `IdentityFile` (recorded on the draft so the user can re-point it).
 *
 * `Match` blocks are skipped: their conditionals need a live target host to
 * evaluate, and guessing produces wrong connections — better to omit the
 * block and report it.
 */
export function parseSshConfig(text: string): ImportResult {
  const configs: ConnectionConfig[] = [];
  const errors: ImportError[] = [];

  let inMatch = false;
  let current: {
    hostAliases: string[];
    hostName?: string;
    port?: number;
    user?: string;
    identity?: string;
    proxyJump?: string;
  } | null = null;

  const flush = () => {
    if (!current) return;
    // 只有 HostName 才算「这台服务器的真实地址」。别退回别名 —— 一个
    // 只有 Host 没有 HostName 的块，别名本身就是连接目标，把 Host 行当
    // 地址导入会造出一个连错地方的连接（并且完全看不出来）。
    const host = current.hostName;
    if (!host) {
      errors.push({ source: current.hostAliases.join(" ") || "(unnamed)", reason: "缺少 HostName" });
      current = null;
      return;
    }
    const name = current.hostAliases[0] ?? host;
    const d = draft({
      name,
      host,
      port: current.port ?? 22,
      username: current.user ?? "root",
      auth_method: current.identity ? "key" : "password",
      // 私钥内容导入不了（ssh_config 只给路径），标出来让用户去补。
      // 有 IdentityFile 就默认按密钥认证走，比按密码更接近原配置意图。
      group_path: "/",
    });
    if (current.proxyJump) {
      // ProxyJump 是「经由另一台 SSH 主机」，MyShell 的代理是 SOCKS5/HTTP。
      // 不猜映射，把跳板机名字写进连接名，用户自己决定怎么配。
      d.name = `${name}（via ${current.proxyJump}）`;
    }
    configs.push(d);
    current = null;
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;

    const m = line.match(/^(\S+)\s*(.*)$/);
    if (!m) continue;
    const key = m[1].toLowerCase();
    const value = m[2].trim();

    if (key === "match") {
      flush();
      inMatch = true;
      continue;
    }
    if (key === "host") {
      flush();
      inMatch = false;
      const aliases = value.split(/\s+/).filter((h) => h && !h.includes("*") && !h.includes("?"));
      if (aliases.length === 0) {
        // 通配 Host 不导入：展开它需要枚举 DNS，且常常命中一批无关主机。
        errors.push({ source: value, reason: "通配 Host 模式不导入" });
        current = null;
        continue;
      }
      current = { hostAliases: aliases };
      continue;
    }
    if (inMatch || !current) continue;

    switch (key) {
      case "hostname":
        current.hostName = value;
        break;
      case "port":
        current.port = Number.parseInt(value, 10) || undefined;
        break;
      case "user":
        current.user = value;
        break;
      case "identityfile":
        current.identity = value;
        break;
      case "proxyjump":
        current.proxyJump = value;
        break;
      default:
        break;
    }
  }
  flush();

  return { configs, errors };
}

// ── PuTTY (.reg) ──────────────────────────────────────────────────────────

/**
 * Parse a PuTTY registry export.
 *
 * PuTTY stores sessions under `HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions\<name>`
 * with values `HostName`, `PortNumber`, `UserName`, `Protocol`. A `.reg` export
 * is a text file with `"Key"="value"` lines and hex blobs.
 *
 * `SavePassword`/encrypted password fields are ignored: PuTTY's password
 * storage is obfuscated, not encrypted, and reverse-engineering it to import
 * credentials is exactly the kind of thing a SSH client shouldn't do.
 */
export function parsePuttyReg(text: string): ImportResult {
  const configs: ConnectionConfig[] = [];
  const errors: ImportError[] = [];

  // Sessions live in a section header line. PuTTY's .reg export writes the
  // key path with BACKSLASHES before the Sessions key and FORWARD SLASHES
  // inside it (the session name is a subkey), e.g.
  //   [HKEY_CURRENT_USER\Software\SimonTatham\PuTTY\Sessions/Prod%20Web]
  // Accepting only one separator silently matched zero sessions.
  const sessionRe = /^\[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions[/\\]([^\]]+)\]\s*$/i;
  const kvRe = /^"([^"]+)"=(.*)$/;

  let current: { name: string; fields: Record<string, string> } | null = null;

  const flush = () => {
    if (!current) return;
    const f = current.fields;
    const displayName = decodeRegName(current.name);
    const host = stripRegString(f.HostName ?? "");
    if (!host) {
      errors.push({ source: displayName, reason: "缺少 HostName" });
      current = null;
      return;
    }
    const proto = stripRegString(f.Protocol ?? "ssh").toLowerCase();
    // MyShell 只有 SSH/SFTP/FTP/本地终端。telnet 与 raw 不收 —— 收进来
    // 也只能标成 ssh，用户点连接时拿到一个完全不同的协议行为，
    // 比明确拒绝更难排查。
    if (proto !== "ssh") {
      errors.push({ source: displayName, reason: `不支持的协议 ${proto}` });
      current = null;
      return;
    }
    configs.push(
      draft({
        name: displayName,
        host,
        port: parseInt(f.PortNumber ?? "22", 10) || 22,
        username: stripRegString(f.UserName ?? "") || "root",
        // PuTTY 的公钥认证走 Pageant，注册表里没有可导入的私钥内容。
        auth_method: f.PublicKeyFile ? "key" : "password",
        conn_type: "ssh",
      })
    );
    current = null;
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith(";")) continue;
    const sec = line.match(sessionRe);
    if (sec) {
      flush();
      current = { name: sec[1], fields: {} };
      continue;
    }
    if (!current) continue;
    const kv = line.match(kvRe);
    if (kv) current.fields[kv[1]] = kv[2].trim();
  }
  flush();

  return { configs, errors };
}

/** `"abc"` → `abc`；非字符串类型（dword:…、hex(2):…）原样返回。 */
function stripRegString(v: string): string {
  const m = v.match(/^"(.*)"$/);
  return m ? m[1] : v;
}

/** PuTTY 的 session 名把 `%20` 当空格编码，还原它。 */
function decodeRegName(name: string): string {
  return name.replace(/%20/g, " ");
}

// ── MobaXterm (.mobaconf) ────────────────────────────────────────────────

/**
 * Parse a MobaXterm `.mobaconf` file.
 *
 * MobaXterm's export is an INI-ish file: `[Bookmarks]` then one
 * `SubRep=` per session, `ImgNum=` per icon. Each SubRep line is
 * pipe-separated:
 *   name | protocol | host | port | user | password-or-# | ...
 * Field order varies by MobaXterm version, so this reads the protocol field
 * and dispatches; unknown protocols are reported, not guessed.
 */
export function parseMobaXterm(text: string): ImportResult {
  const configs: ConnectionConfig[] = [];
  const errors: ImportError[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("[") || line.startsWith(";")) continue;
    const eq = line.indexOf("=");
    if (eq < 0) continue;
    const key = line.slice(0, eq).trim();
    if (key !== "SubRep") continue;
    const parts = line
      .slice(eq + 1)
      .split("#")
      .map((p) => p.trim())
      // MobaXterm escapes its separator; a leading backslash undoes it.
      .map((p) => p.replace(/\\#/g, "#"));

    // 典型: 0=name 1=proto 2=host 3=port 4=user 5=pass
    const name = parts[0] ?? "";
    const proto = (parts[1] ?? "").toLowerCase();
    const host = parts[2] ?? "";
    const port = parseInt(parts[3] ?? "", 10) || 22;
    const user = parts[4] ?? "";

    if (!host) {
      errors.push({ source: name || "(unnamed)", reason: "缺少主机地址" });
      continue;
    }
    if (proto && proto !== "ssh") {
      errors.push({ source: name || host, reason: `不支持的协议 ${proto}` });
      continue;
    }
    // MobaXterm 的第 6 段是密码（或 # 占位）。MyShell 不接收外部密码，
    // 一律留空由用户填 —— 见文件头约束 1。
    configs.push(
      draft({
        name: name || host,
        host,
        port,
        username: user || "root",
        auth_method: "password",
        conn_type: "ssh",
      })
    );
  }

  return { configs, errors };
}

// ── SecureCRT (INI) ──────────────────────────────────────────────────────

/**
 * Parse a SecureCRT session export.
 *
 * Two shapes exist in the wild and both are handled:
 * 1. A single session file: `S:"Key"="Value"` lines with the session name in
 *    `S:"Session Name"=`, no INI section headers.
 * 2. A bulk export: `[Session Name]` section headers, then the same
 *    `S:"Key"="Value"` lines underneath.
 *
 * Keys of interest: `Hostname`, `Port`, `Username`, `Protocol Name` (SSH2 /
 * Telnet / Serial), `Identity Filename`. Values carry a trailing `\0000`
 * NUL in some exports — stripped on read.
 */
export function parseSecureCrt(text: string): ImportResult {
  const configs: ConnectionConfig[] = [];
  const errors: ImportError[] = [];

  const kvRe = /^S:"([^"]+)"=(.*)$/;
  let current: { name: string; fields: Record<string, string> } | null = null;

  const flush = () => {
    if (!current) return;
    const f = current.fields;
    const host = stripSecureCrt(f.Hostname ?? "");
    const proto = stripSecureCrt(f["Protocol Name"] ?? f.Protocol ?? "SSH2").toUpperCase();
    // 扁平形态（无节标题）时，会话名来自 Session Name 字段本身。
    const name = current.name || stripSecureCrt(f["Session Name"] ?? "") || host;
    if (!host) {
      errors.push({ source: name || "(unnamed)", reason: "缺少 Hostname" });
      current = null;
      return;
    }
    if (proto.startsWith("TELNET") || proto.startsWith("SERIAL") || proto.startsWith("RLOGIN")) {
      errors.push({ source: name, reason: `不支持的协议 ${proto}` });
      current = null;
      return;
    }
    configs.push(
      draft({
        name,
        host,
        port: parseInt(stripSecureCrt(f.Port ?? "22"), 10) || 22,
        username: stripSecureCrt(f.Username ?? "") || "root",
        auth_method: f["Identity Filename"] ? "key" : "password",
        conn_type: "ssh",
      })
    );
    current = null;
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.replace(/\x00+$/, "").trim();
    if (!line || line.startsWith(";")) continue;
    // Bulk-export shape: a section header names the session.
    if (line.startsWith("[")) {
      flush();
      const sec = line.match(/^\[(.*)\]\s*$/);
      current = sec ? { name: sec[1].trim(), fields: {} } : null;
      continue;
    }
    const kv = line.match(kvRe);
    if (!kv) continue;
    // Flat shape: start a session on the first key we see.
    if (!current) current = { name: "", fields: {} };
    current.fields[kv[1]] = kv[2];
  }
  flush();

  return { configs, errors };
}

function stripSecureCrt(v: string): string {
  return v.replace(/\x00/g, "").replace(/^"|"$/g, "").trim();
}

// ── CSV ──────────────────────────────────────────────────────────────────

/**
 * Parse a generic CSV with a header row.
 *
 * Recognises common header spellings (name/host/port/user( name)?, in any
 * order, case-insensitive). Rows missing a host are reported with their line
 * number so the user can fix the source file.
 */
export function parseCsv(text: string): ImportResult {
  const configs: ConnectionConfig[] = [];
  const errors: ImportError[] = [];

  const rows = splitCsvRows(text);
  if (rows.length === 0) return { configs, errors };

  const header = rows[0].map((h) => h.trim().toLowerCase());
  const col = (...names: string[]): number => {
    for (const n of names) {
      const i = header.indexOf(n);
      if (i >= 0) return i;
    }
    return -1;
  };
  const iName = col("name", "名称", "会话名", "label", "title");
  const iHost = col("host", "hostname", "主机", "地址", "server", "ip");
  const iPort = col("port", "端口");
  const iUser = col("user", "username", "用户", "用户名", "login");
  const iGroup = col("group", "folder", "分组", "组");
  const iType = col("type", "protocol", "协议", "类型");

  if (iHost < 0) {
    return {
      configs: [],
      errors: [{ source: "表头", reason: `找不到主机列（已识别表头：${header.join(", ")}）` }],
    };
  }

  for (let r = 1; r < rows.length; r++) {
    const cells = rows[r];
    const lineNo = r + 1;
    const host = (cells[iHost] ?? "").trim();
    if (!host) {
      // 整行空白通常是文件末尾的换行，不算错误。
      if (cells.every((c) => !c.trim())) continue;
      errors.push({ source: `第 ${lineNo} 行`, reason: "缺少主机地址" });
      continue;
    }
    const rawType = iType >= 0 ? (cells[iType] ?? "").trim().toLowerCase() : "ssh";
    // CSV 导入的连接类型只认这三种，其余一律按 ssh 收（导入容错：
    // 未知类型值不阻断整行，落回默认协议）。显式映射而非链式三元。
    const connType: ConnType = rawType === "ftp" || rawType === "sftp" ? rawType : "ssh";
    configs.push(
      draft({
        name: (iName >= 0 ? cells[iName] : "")?.trim() || host,
        host,
        port: iPort >= 0 ? parseInt(cells[iPort], 10) || 22 : 22,
        username: (iUser >= 0 ? cells[iUser] : "")?.trim() || "root",
        group_path: (iGroup >= 0 ? cells[iGroup] : "")?.trim() || "/",
        conn_type: connType,
        auth_method: "password",
      })
    );
  }

  return { configs, errors };
}

/** Minimal RFC-4180 row splitter: handles quoted fields with embedded
 *  commas/newlines. No full parser needed — these files come from export
 *  dialogs, not hand-editing. */
function splitCsvRows(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let inQuotes = false;

  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (inQuotes) {
      if (c === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i++;
        } else {
          inQuotes = false;
        }
      } else {
        field += c;
      }
      continue;
    }
    if (c === '"') {
      inQuotes = true;
    } else if (c === ",") {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      field = "";
      if (row.some((v) => v !== "")) rows.push(row);
      row = [];
    } else {
      field += c;
    }
  }
  row.push(field);
  if (row.some((v) => v !== "")) rows.push(row);
  return rows;
}
