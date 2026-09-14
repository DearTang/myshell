import { useCallback, useEffect, useRef, useState } from "react";
import type { ConnectionConfig } from "../api";
import { moveConnection } from "../api";

// ── Tunables ───────────────────────────────────────────────────────────────
/** How long the user must press before drag activates. 600ms is the mobile
 * long-press standard; combined with MOVE_CANCEL_PX, a normal click or
 * double-click can never trigger a drag. */
const LONG_PRESS_MS = 600;
/** Pointer displacement beyond which a pending long-press is cancelled
 * (treated as a scroll/pan intent rather than a hold). */
const MOVE_CANCEL_PX = 5;

// ── Types ───────────────────────────────────────────────────────────────────
export interface DragState {
  connId: string;
  connName: string;
  /** Folder the connection currently lives in ("/" = root). Used to no-op a
   * drop onto the connection's own current folder. */
  connGroupPath: string;
  /** Folder under the cursor right now (null = over empty area / no folder). */
  hoverFolderPath: string | null;
}

export interface UseConnectionDragOptions {
  /** Called after a successful move so the host can auto-expand the target
   * folder and refresh the connection tree. */
  onMoved: (targetFolderPath: string) => void;
  /** Called when moveConnection rejects, so the host can surface an error. */
  onMoveError: (connId: string, err: unknown) => void;
}

export interface UseConnectionDrag {
  /** null when idle. Non-null while a drag is active — the host should switch
   * to a compact "folders only" drop-target view. */
  dragState: DragState | null;
  /** Attach to a ConnRow's onPointerDown. Returns early for non-primary
   * buttons or while a drag/move is already in flight. */
  beginDrag: (conn: ConnectionConfig, e: React.PointerEvent) => void;
}

// ── Hook ────────────────────────────────────────────────────────────────────
export function useConnectionDrag({
  onMoved,
  onMoveError,
}: UseConnectionDragOptions): UseConnectionDrag {
  const [dragState, setDragState] = useState<DragState | null>(null);

  // Mirror the latest callbacks into refs so `beginDrag` (stable identity)
  // always invokes the current versions without re-binding ConnRow props.
  const onMovedRef = useRef(onMoved);
  const onMoveErrorRef = useRef(onMoveError);
  onMovedRef.current = onMoved;
  onMoveErrorRef.current = onMoveError;

  // A drag is active from pointerdown until drop/cancel, even during the
  // 600ms pre-activation window — blocks a second pointerdown from stacking.
  const activeRef = useRef(false);
  // Guards a move already in flight so a rapid second drop can't double-fire.
  const isMovingRef = useRef(false);

  const beginDrag = useCallback(
    (conn: ConnectionConfig, e: React.PointerEvent) => {
      // Only primary button; ignore right/middle/synthetic.
      if (e.button !== 0) return;
      // Never start a new drag while one is active or a move is in flight.
      if (activeRef.current || isMovingRef.current) return;

      const el = e.currentTarget as HTMLElement;
      const pointerId = e.pointerId;
      const startX = e.clientX;
      const startY = e.clientY;
      const connGroupPath = conn.group_path || "/";
      // `dragActivated` flips true when the timer fires (Phase A → Phase B).
      // cleanupA uses it to know whether it still owns pointer capture.
      let dragActivated = false;
      // Declared with `let` so cleanupA/cleanupA's timer branch can null it.
      let timerId: number | null = null;

      activeRef.current = true;

      try {
        el.setPointerCapture(pointerId);
      } catch {
        // setPointerCapture throws if the element is detached mid-press; the
        // element-level listeners below are still attached and will still fire.
      }

      // ── Phase A: long-press detection (listeners on the captured element) ──
      const cleanupA = () => {
        if (timerId !== null) {
          window.clearTimeout(timerId);
          timerId = null;
        }
        el.removeEventListener("pointermove", onMoveA);
        el.removeEventListener("pointerup", onUpA);
        el.removeEventListener("pointercancel", onCancelA);
        window.removeEventListener("blur", onBlurA);
        document.removeEventListener("keydown", onKeyA);
        // Release capture only while still in Phase A; once activate() has
        // handed off to Phase B it releases capture itself.
        if (!dragActivated && el.hasPointerCapture(pointerId)) {
          try {
            el.releasePointerCapture(pointerId);
          } catch {
            /* ignore */
          }
        }
        activeRef.current = false;
      };
      const onMoveA = (ev: PointerEvent) => {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) > MOVE_CANCEL_PX) {
          cleanupA();
        }
      };
      const onUpA = () => cleanupA();
      const onCancelA = () => cleanupA();
      const onBlurA = () => cleanupA();
      const onKeyA = (ke: KeyboardEvent) => {
        if (ke.key === "Escape") cleanupA();
      };

      // ── Phase B: dragging (listeners on document for hit-testing) ──────────
      const activate = () => {
        dragActivated = true;
        // Tear down Phase A listeners + release element capture so document
        // receives the stream of pointer events.
        cleanupA();
        try {
          if (el.hasPointerCapture(pointerId)) el.releasePointerCapture(pointerId);
        } catch {
          /* ignore */
        }
        activeRef.current = true;

        // Body affordances: grab cursor + suppress text selection while held.
        const prevCursor = document.body.style.cursor;
        const prevUserSelect = document.body.style.userSelect;
        document.body.style.cursor = "grabbing";
        document.body.style.userSelect = "none";

        setDragState({
          connId: conn.id,
          connName: conn.name,
          connGroupPath,
          hoverFolderPath: null,
        });

        const hitTestFolderPath = (ev: PointerEvent): string | null => {
          // pointerEvents:none on the dragged ConnRow (set by the host) ensures
          // elementFromPoint returns the folder beneath, not the dragged row.
          const hit = document.elementFromPoint(ev.clientX, ev.clientY);
          const folder = hit && typeof hit.closest === "function"
            ? hit.closest<HTMLElement>("[data-folder-path]")
            : null;
          return folder?.getAttribute("data-folder-path") ?? null;
        };

        const onMoveB = (ev: PointerEvent) => {
          const path = hitTestFolderPath(ev);
          // setState only when the hovered folder changes — avoids a re-render
          // storm (the whole folder list re-renders on each hover change).
          setDragState((s) =>
            s && s.hoverFolderPath !== path ? { ...s, hoverFolderPath: path } : s
          );
        };

        const detachB = () => {
          document.removeEventListener("pointermove", onMoveB);
          document.removeEventListener("pointerup", onUpB);
          document.removeEventListener("pointercancel", onCancelB);
          document.removeEventListener("keydown", onKeyB);
          window.removeEventListener("blur", onBlurB);
          document.body.style.cursor = prevCursor;
          document.body.style.userSelect = prevUserSelect;
          setDragState(null);
          activeRef.current = false;
        };

        const onCancelB = () => detachB();
        const onBlurB = () => detachB();
        const onKeyB = (ke: KeyboardEvent) => {
          if (ke.key === "Escape") detachB();
        };

        const onUpB = async (ev: PointerEvent) => {
          const targetPath = hitTestFolderPath(ev);
          detachB();
          // Empty area or the connection's own current folder → no-op.
          if (!targetPath || targetPath === connGroupPath) return;
          isMovingRef.current = true;
          try {
            await moveConnection(conn.id, targetPath);
            onMovedRef.current(targetPath);
          } catch (err) {
            onMoveErrorRef.current(conn.id, err);
          } finally {
            isMovingRef.current = false;
          }
        };

        document.addEventListener("pointermove", onMoveB);
        document.addEventListener("pointerup", onUpB);
        document.addEventListener("pointercancel", onCancelB);
        document.addEventListener("keydown", onKeyB);
        window.addEventListener("blur", onBlurB);
      };

      el.addEventListener("pointermove", onMoveA);
      el.addEventListener("pointerup", onUpA);
      el.addEventListener("pointercancel", onCancelA);
      window.addEventListener("blur", onBlurA);
      document.addEventListener("keydown", onKeyA);

      timerId = window.setTimeout(activate, LONG_PRESS_MS);
    },
    []
  );

  // Safety net: if the component unmounts mid-drag (e.g. tree reload swapped
  // the ConnRow), the document listeners leak. We can't easily reach the
  // closures from here, so this only clears the React state; the body style
  // is restored by the next pointerup/cancel in practice. Acceptable for v1.
  useEffect(() => {
    return () => {
      setDragState(null);
      activeRef.current = false;
      isMovingRef.current = false;
    };
  }, []);

  return { dragState, beginDrag };
}
