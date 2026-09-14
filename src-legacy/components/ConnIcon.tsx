import type { CSSProperties } from "react";
import type { ConnType } from "../api";

/**
 * Connection-type icon rendered from the bundled iconfont (iconfont.cn).
 *
 * Glyphs (see src/assets/iconfont/iconfont.css):
 *   ssh   → icon-fuwuqi  (服务器)
 *   sftp  → icon-SFTP
 *   ftp   → icon-ftp
 *   local → icon-diannao (电脑)
 *
 * The icon inherits its color from `currentColor`, so callers tint it via
 * `style.color` / CSS `color`. `size` controls font-size in px.
 *
 * Replaces the previous emoji set (🖥️ / 📁 / 📤 / 💻) which rendered
 * inconsistently across platforms.
 */
const CONN_ICON_CLASS: Record<ConnType, string> = {
  ssh: "icon-fuwuqi",
  sftp: "icon-SFTP",
  ftp: "icon-ftp",
  local: "icon-diannao",
};

/**
 * Semantic tint per connection type, mirrored across sidebar / tab bar /
 * connection dialog so each surface reads the same color cue. Callers can
 * override via `style.color`.
 */
export const CONN_COLOR: Record<ConnType, string> = {
  ssh: "var(--accent-primary)",
  sftp: "var(--accent-secondary)",
  ftp: "var(--warning)",
  local: "var(--text-secondary)",
};

interface Props {
  connType: ConnType;
  size?: number;
  className?: string;
  style?: CSSProperties;
  title?: string;
}

export function ConnIcon({ connType, size = 16, className, style, title }: Props) {
  const cls = CONN_ICON_CLASS[connType] || CONN_ICON_CLASS.ssh;
  return (
    <i
      className={`iconfont ${cls}${className ? ` ${className}` : ""}`}
      title={title}
      style={{ fontSize: size, color: CONN_COLOR[connType] || CONN_COLOR.ssh, ...style }}
      aria-hidden={title ? undefined : true}
    />
  );
}
