import { useEffect, useMemo, useRef, useState } from "react";
import { listSystemFonts } from "../api";

/**
 * Font picker — a custom combobox with fuzzy search over the host's installed
 * font families (fetched once via the `list_system_fonts` Tauri command and
 * cached at module scope so the settings panel and connection dialog share a
 * single round-trip).
 *
 * The user types to filter, picks from a themed dropdown, or just types any
 * family name free-form — the system list can be incomplete (or enumeration can
 * fail), so free-text must keep working.
 *
 * Replaces the previous native <datalist> implementation, which (a) couldn't be
 * styled to match the app and (b) only offered prefix matching instead of fuzzy
 * search.
 *
 * Used by the global terminal-font setting and the per-connection override.
 */

let cachedFonts: string[] | null = null;
let fetchPromise: Promise<string[]> | null = null;

function loadFonts(): Promise<string[]> {
  if (cachedFonts) return Promise.resolve(cachedFonts);
  if (!fetchPromise) {
    fetchPromise = listSystemFonts()
      .then((f) => {
        cachedFonts = f;
        return f;
      })
      .catch(() => {
        cachedFonts = [];
        return [];
      });
  }
  return fetchPromise;
}

/** Cap on rendered options — keeps the DOM light on machines with huge font sets. */
const MAX_RESULTS = 200;

interface Props {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
}

const inputStyle: React.CSSProperties = {
  width: "100%",
  // right padding leaves room for the dropdown caret glyph
  padding: "10px 30px 10px 12px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  outline: "none",
  fontFamily: "inherit",
  transition:
    "border-color var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out)",
};

export function FontField({ value, onChange, placeholder }: Props) {
  const [fonts, setFonts] = useState<string[]>(cachedFonts ?? []);
  const [loaded, setLoaded] = useState<boolean>(cachedFonts !== null);
  const [open, setOpen] = useState(false);
  // `query` drives filtering, kept separate from `value` so that re-focusing an
  // already-chosen font still shows the full list instead of filtering down to
  // the current value. Cleared on pick and on external clear.
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    loadFonts().then((f) => {
      if (alive) {
        setFonts(f);
        setLoaded(true);
      }
    });
    return () => {
      alive = false;
    };
  }, []);

  // Reset the filter when the field is cleared externally (e.g. dialog reopen).
  useEffect(() => {
    if (value === "") setQuery("");
  }, [value]);

  const tokens = useMemo(
    () => query.trim().toLowerCase().split(/\s+/).filter(Boolean),
    [query]
  );

  // Fuzzy filter: every whitespace-separated token must appear in the family
  // name (case-insensitive), in any order — so "nerd mono" matches
  // "JetBrainsMono Nerd Font Mono". Empty query shows the whole (sorted) list.
  const filtered = useMemo(() => {
    if (tokens.length === 0) return fonts.slice(0, MAX_RESULTS);
    return fonts
      .filter((f) => tokens.every((t) => f.toLowerCase().includes(t)))
      .slice(0, MAX_RESULTS);
  }, [fonts, tokens]);

  // Reset highlight to the top whenever the result set changes.
  useEffect(() => {
    setHighlight(0);
  }, [filtered]);

  // Keep the highlighted option in view during keyboard navigation.
  useEffect(() => {
    if (!open || !listRef.current) return;
    const el = listRef.current.children[highlight] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [highlight, open]);

  function choose(font: string) {
    onChange(font);
    setQuery("");
    setOpen(false);
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      if (!open) {
        setOpen(true);
        return;
      }
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      if (!open) return;
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === "Enter") {
      if (open && filtered[highlight]) {
        e.preventDefault();
        choose(filtered[highlight]);
      }
    } else if (e.key === "Escape") {
      if (open) {
        e.preventDefault();
        setOpen(false);
      }
    }
  }

  return (
    <div style={{ position: "relative" }}>
      <input
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setQuery(e.target.value);
        }}
        onFocus={(e) => {
          e.currentTarget.style.borderColor = "var(--accent-primary)";
          e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
          setOpen(true);
        }}
        onBlur={(e) => {
          e.currentTarget.style.borderColor = "var(--border-default)";
          e.currentTarget.style.boxShadow = "none";
          setOpen(false);
        }}
        onClick={() => setOpen(true)}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        style={inputStyle}
      />
      {/* caret — signals this is a pickable field, not plain free text */}
      <span
        aria-hidden
        style={{
          position: "absolute",
          right: 10,
          top: "50%",
          transform: "translateY(-50%)",
          fontSize: 11,
          color: "var(--text-tertiary)",
          pointerEvents: "none",
        }}
      >
        ▾
      </span>

      {open && (
        <div
          ref={listRef}
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            left: 0,
            right: 0,
            maxHeight: 240,
            overflowY: "auto",
            background: "var(--bg-elevated)",
            border: "1px solid var(--border-emphasis)",
            borderRadius: "var(--radius-md)",
            boxShadow: "var(--shadow-xl)",
            zIndex: 100,
            padding: 4,
          }}
        >
          {!loaded ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-muted)" }}>
              加载字体列表…
            </div>
          ) : filtered.length === 0 ? (
            <div style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-muted)" }}>
              无匹配字体，可自定义输入
            </div>
          ) : (
            <>
              {filtered.map((f, i) => {
                const active = i === highlight;
                return (
                  <div
                    key={f}
                    // preventDefault keeps the input focused so onClick fires
                    // (otherwise input blur would close the dropdown first).
                    onMouseDown={(e) => e.preventDefault()}
                    onMouseEnter={() => setHighlight(i)}
                    onClick={() => choose(f)}
                    title={f}
                    style={{
                      padding: "7px 10px",
                      fontSize: 13,
                      borderRadius: "var(--radius-sm)",
                      cursor: "pointer",
                      background: active ? "var(--accent-primary-muted)" : "transparent",
                      color: active ? "var(--accent-primary)" : "var(--text-secondary)",
                      fontWeight: active ? 600 : 400,
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    }}
                  >
                    {renderHighlighted(f, tokens)}
                  </div>
                );
              })}
              {tokens.length > 0 && filtered.length === MAX_RESULTS && (
                <div
                  style={{
                    padding: "6px 10px",
                    fontSize: 11,
                    color: "var(--text-muted)",
                    borderTop: "1px solid var(--border-subtle)",
                    marginTop: 2,
                  }}
                >
                  仅显示前 {MAX_RESULTS} 条，输入更多关键字可缩小范围
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Split `name` into matched/unmatched fragments against the search tokens and
 * bold+recolor the matched spans so fuzzy hits stay legible. Returns the raw
 * name when there is nothing to highlight (empty query / no overlap).
 */
function renderHighlighted(name: string, tokens: string[]): React.ReactNode {
  if (tokens.length === 0) return name;
  const lower = name.toLowerCase();
  const ranges: Array<[number, number]> = [];
  for (const t of tokens) {
    let from = 0;
    while (from <= lower.length) {
      const idx = lower.indexOf(t, from);
      if (idx === -1) break;
      ranges.push([idx, idx + t.length]);
      from = idx + t.length;
    }
  }
  if (ranges.length === 0) return name;

  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) {
      last[1] = Math.max(last[1], r[1]);
    } else {
      merged.push([r[0], r[1]]);
    }
  }

  const parts: React.ReactNode[] = [];
  let pos = 0;
  merged.forEach((r, i) => {
    if (r[0] > pos) parts.push(<span key={`t${i}`}>{name.slice(pos, r[0])}</span>);
    parts.push(
      <span key={`m${i}`} style={{ color: "var(--text-primary)", fontWeight: 700 }}>
        {name.slice(r[0], r[1])}
      </span>
    );
    pos = r[1];
  });
  if (pos < name.length) parts.push(<span key="end">{name.slice(pos)}</span>);
  return <>{parts}</>;
}