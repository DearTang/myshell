import { open } from "@tauri-apps/plugin-dialog";
// zmodem.js ships CommonJS; Vite's esbuild interop gives us the default export.
// Disable TS types via .d.ts in env.d.ts since the package has no @types.
import Zmodem from "zmodem.js";

import {
  rzOpenRead,
  rzReadChunk,
  rzClose,
  szOpenWrite,
  szWriteChunk,
  szClose,
  sshSendZmodem,
  sshSendZmodemAbort,
} from "./api";

const CHUNK_SIZE = 8192; // matches zmodem.js MAX_CHUNK_LENGTH

export interface ZmodemStatus {
  active: boolean;
  direction: "upload" | "download" | null;
  currentFile: string;
  bytesTransferred: number;
  bytesTotal: number;
  speedBps: number;
  error: string | null;
}

export type StatusListener = (status: ZmodemStatus) => void;

/**
 * Bridges Rust-emitted ZMODEM bytes to zmodem.js's Session API and
 * coordinates file IO (Tauri dialog + streaming read/write via Rust).
 *
 * Lifecycle: instantiated per sessionId by TerminalPanel. `feed()` is
 * called for each `zmodem_raw` event. The first call parses the session
 * role (Send = remote rz wants files from us / Receive = remote sz is
 * pushing files to us). Subsequent calls just `consume()` into the
 * active session.
 */
export class ZmodemBridge {
  private sessionId: string;
  private statusListeners = new Set<StatusListener>();
  private session: any = null;
  private aborted = false;
  private writeHandleId: string | null = null;
  private readHandleId: string | null = null;
  // Bytes accumulated before Session.parse succeeds — the initial ZMODEM
  // header may arrive split across multiple `zmodem_raw` events.
  private pendingHeader: number[] = [];
  // Cached target directory for multi-file sz downloads. Set on the first
  // offer so we don't prompt the user once per file.
  private downloadDir: string | null = null;
  // Once the user cancels a download, skip remaining offers silently.
  private downloadCancelled = false;

  private status: ZmodemStatus = {
    active: false,
    direction: null,
    currentFile: "",
    bytesTransferred: 0,
    bytesTotal: 0,
    speedBps: 0,
    error: null,
  };

  // Speed sampling — reset per file
  private speedStartTime = 0;
  private speedStartBytes = 0;
  private speedLastSample = 0;

  constructor(sessionId: string) {
    this.sessionId = sessionId;
  }

  onStatus(listener: StatusListener): () => void {
    this.statusListeners.add(listener);
    listener(this.status);
    return () => {
      this.statusListeners.delete(listener);
    };
  }

  private emitStatus(): void {
    for (const l of this.statusListeners) l(this.status);
  }

  private patchStatus(patch: Partial<ZmodemStatus>): void {
    this.status = { ...this.status, ...patch };
    this.emitStatus();
  }

  /**
   * Feed a chunk of raw ZMODEM bytes from the Rust backend.
   * Accumulates bytes until the session header parses, then dispatches
   * subsequent chunks straight to the active session.
   */
  feed(bytes: Uint8Array): void {
    if (this.aborted) return;
    if (bytes.length === 0) return;

    if (!this.session) {
      // The first frame can land split across multiple `zmodem_raw`
      // events (PTY buffer boundaries). Accumulate until the hex header
      // parses, then hand the full prelude to the new session.
      for (const b of bytes) this.pendingHeader.push(b);
      const session = Zmodem.Session.parse(this.pendingHeader);
      if (!session) return;
      this.session = session;
      const prelude = this.pendingHeader;
      this.pendingHeader = [];
      this.setupSession(session);
      session.consume(prelude);
    } else {
      this.session.consume(Array.from(bytes));
    }
  }

  private setupSession(session: any): void {
    session.set_sender((octets: number[]) => {
      // zmodem.js produced protocol bytes — push them into the SSH channel.
      sshSendZmodem(this.sessionId, new Uint8Array(octets)).catch((e) => {
        console.error("sshSendZmodem failed:", e);
      });
    });

    session.on("session_end", () => {
      this.patchStatus({
        active: false,
        direction: null,
        currentFile: "",
        bytesTransferred: 0,
        bytesTotal: 0,
        speedBps: 0,
      });
    });

    if (session.type === "receive") {
      // Remote ran `sz` — we're the receiver, files come to us.
      void this.handleDownload(session);
    } else {
      // Remote ran `rz` — we're the sender, push files out.
      void this.handleUpload(session);
    }
  }

  /**
   * sz download: stream offered files to disk.
   *
   * UX policy:
   *   - Always prompt ONCE for a target directory on the first offer, dump
   *     all subsequent offers there. lrzsz doesn't reliably fill
   *     `files_remaining`, so we can't distinguish single-file from batch
   *     upfront — a single directory picker handles both cases uniformly.
   *   - Cancel the directory picker → skip remaining offers silently.
   */
  private async handleDownload(session: any): Promise<void> {
    this.patchStatus({ active: true, direction: "download" });

    let pendingResolve: ((o: any) => void) | null = null;
    const waitForNextOffer = () =>
      new Promise<any>((resolve) => {
        pendingResolve = resolve;
      });
    const onOffer = (offer: any) => {
      const r = pendingResolve;
      pendingResolve = null;
      r?.(offer);
    };
    const onSessionEnd = () => {
      const r = pendingResolve;
      pendingResolve = null;
      r?.(null);
    };
    session.on("offer", onOffer);
    session.on("session_end", onSessionEnd);

    try {
      let offer: any = await session.start();
      while (offer) {
        const details = offer.get_details();
        const name: string = details.name || "file";

        let targetPath: string | null;
        if (this.downloadCancelled) {
          targetPath = null;
        } else {
          if (this.downloadDir === null) {
            const dir = await open({ directory: true });
            this.downloadDir =
              typeof dir === "string" && dir.length > 0 ? dir : null;
            if (this.downloadDir === null) {
              this.downloadCancelled = true;
            }
          }
          targetPath =
            this.downloadDir !== null
              ? joinPath(this.downloadDir, name)
              : null;
        }

        if (!targetPath) {
          offer.skip();
        } else {
          await this.receiveFile(offer, targetPath, name, details.size ?? 0);
        }
        offer = await waitForNextOffer();
      }
    } catch (err) {
      console.error("sz download failed:", err);
      this.patchStatus({ error: String(err) });
    } finally {
      try {
        session.off("offer", onOffer);
        session.off("session_end", onSessionEnd);
      } catch {
        // off() throws if handler was already removed — ignore.
      }
    }
  }

  private async receiveFile(
    offer: any,
    path: string,
    name: string,
    size: number
  ): Promise<void> {
    this.patchStatus({
      currentFile: name,
      bytesTotal: size,
      bytesTransferred: 0,
      speedBps: 0,
    });
    this.speedStartTime = Date.now();
    this.speedStartBytes = 0;
    this.speedLastSample = 0;

    const { id, existingSize } = await szOpenWrite(path);
    this.writeHandleId = id;

    // Resume: if the target file already has bytes on disk, ask the sender
    // to skip what we already have. zmodem.js + lrzsz support this via
    // ZRPOS offset negotiation.
    const resumeOffset = existingSize > 0 && existingSize < size ? existingSize : 0;
    if (resumeOffset > 0) {
      this.patchStatus({ bytesTransferred: resumeOffset });
      this.speedStartBytes = resumeOffset;
    }

    offer.on("input", (payload: number[] | Uint8Array) => {
      const bytes =
        payload instanceof Uint8Array ? payload : new Uint8Array(payload);
      const offset = this.status.bytesTransferred;
      szWriteChunk(id, offset, bytes).catch((e) => {
        console.error("sz_write_chunk failed:", e);
      });
      this.patchStatus({
        bytesTransferred: offset + bytes.length,
      });
      this.sampleSpeed();
    });

    try {
      await offer.accept({ offset: resumeOffset });
    } finally {
      if (this.writeHandleId) {
        await szClose(this.writeHandleId);
        this.writeHandleId = null;
      }
    }
  }

  /** rz upload: prompt for one or more source files, stream each to peer. */
  private async handleUpload(session: any): Promise<void> {
    this.patchStatus({ active: true, direction: "upload" });

    try {
      const selected = await open({ multiple: true });
      const paths: string[] = !selected
        ? []
        : Array.isArray(selected)
        ? selected
        : [selected];

      if (paths.length === 0) {
        session.close();
        return;
      }

      for (let i = 0; i < paths.length; i++) {
        await this.sendFile(session, paths[i], paths.length - i);
      }
      await session.close();
    } catch (err) {
      console.error("rz upload failed:", err);
      this.patchStatus({ error: String(err) });
    }
  }

  private async sendFile(
    session: any,
    path: string,
    filesRemaining: number
  ): Promise<void> {
    const fileName = path.split(/[\\/]/).pop() || "file";
    const { id, size, mtime } = await rzOpenRead(path);
    this.readHandleId = id;

    this.patchStatus({
      currentFile: fileName,
      bytesTotal: size,
      bytesTransferred: 0,
      speedBps: 0,
    });
    this.speedStartTime = Date.now();
    this.speedStartBytes = 0;
    this.speedLastSample = 0;

    const transfer = await session.send_offer({
      name: fileName,
      size,
      mtime: new Date(mtime * 1000),
      files_remaining: filesRemaining,
      bytes_remaining: size,
    });

    if (!transfer) {
      // Peer refused/skipped the offer.
      if (this.readHandleId) {
        await rzClose(this.readHandleId);
        this.readHandleId = null;
      }
      return;
    }

    // Resume support: if peer sent ZRPOS with a non-zero offset, start
    // reading from there instead of the file head.
    let offset = transfer.get_offset();
    if (offset > 0) {
      this.patchStatus({ bytesTransferred: offset });
      this.speedStartBytes = offset;
    }
    try {
      while (offset < size) {
        const len = Math.min(CHUNK_SIZE, size - offset);
        const chunk = await rzReadChunk(this.readHandleId!, offset, len);
        if (chunk.length === 0) break;
        transfer.send(chunk);
        offset += chunk.length;
        this.patchStatus({ bytesTransferred: offset });
        this.sampleSpeed();
        // Yield so the UI can repaint and the JS event loop can drain
        // any incoming acks from zmodem.js's sender.
        await new Promise((r) => setTimeout(r, 0));
      }
      await transfer.end(new Uint8Array(0));
    } finally {
      if (this.readHandleId) {
        await rzClose(this.readHandleId);
        this.readHandleId = null;
      }
    }
  }

  private sampleSpeed(): void {
    const now = Date.now();
    if (now - this.speedLastSample < 500) return;
    this.speedLastSample = now;
    const elapsed = (now - this.speedStartTime) / 1000;
    if (elapsed > 0) {
      this.patchStatus({
        speedBps: this.status.bytesTransferred / elapsed,
      });
    }
  }

  /** User clicked cancel. Fire the lrzsz abort sequence and tear down. */
  abort(): void {
    if (this.aborted) return;
    this.aborted = true;

    if (this.writeHandleId) {
      szClose(this.writeHandleId).catch(() => {});
      this.writeHandleId = null;
    }
    if (this.readHandleId) {
      rzClose(this.readHandleId).catch(() => {});
      this.readHandleId = null;
    }

    if (this.session) {
      try {
        this.session.abort();
      } catch {
        // Session may already be ended — fall back to raw abort bytes.
        sshSendZmodemAbort(this.sessionId).catch(() => {});
      }
    } else {
      sshSendZmodemAbort(this.sessionId).catch(() => {});
    }

    this.patchStatus({
      active: false,
      direction: null,
      currentFile: "",
      bytesTransferred: 0,
      bytesTotal: 0,
      speedBps: 0,
    });
  }

  /**
   * Rust detected ZFIN or 5× CAN — force-reset so the UI returns to terminal
   * mode and the bridge can accept the next sz/rz session. The bridge is
   * per-sessionId (lives for the whole SSH tab) but reusable across
   * sequential transfers, so we clear all per-transfer state here.
   */
  reset(): void {
    if (this.writeHandleId) {
      szClose(this.writeHandleId).catch(() => {});
      this.writeHandleId = null;
    }
    if (this.readHandleId) {
      rzClose(this.readHandleId).catch(() => {});
      this.readHandleId = null;
    }

    this.session = null;
    this.pendingHeader = [];
    this.downloadDir = null;
    this.downloadCancelled = false;
    this.aborted = false;

    this.patchStatus({
      active: false,
      direction: null,
      currentFile: "",
      bytesTransferred: 0,
      bytesTotal: 0,
      speedBps: 0,
    });
  }
}

/**
 * Join a directory and a filename with the correct separator. Local FS is
 * Windows (backslash) but the directory picker can return forward-slash
 * paths depending on how the user navigated; pick the separator that
 * matches the input. Strip any path components from `name` so a remote
 * `sz ./sub/file` can't write outside the chosen directory.
 */
function joinPath(dir: string, name: string): string {
  const safeName = name.split(/[\\/]/).pop() || name;
  const sep = dir.includes("/") && !dir.includes("\\") ? "/" : "\\";
  const trimmed = dir.endsWith(sep) ? dir.slice(0, -1) : dir;
  return `${trimmed}${sep}${safeName}`;
}
