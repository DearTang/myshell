import { useId } from "react";

interface BrandLogoProps {
  /** Render size in px (square). */
  size?: number;
  /** Add a soft brand-colored glow — use for large hero placements. */
  glow?: boolean;
}

/**
 * Inline-vector `>_` brand mark — the same mark as the app icon
 * (Aurora Prompt). Uses the theme accent CSS variables so it adapts
 * to light/dark automatically. No squircle tile (that belongs to the
 * OS icon, not the in-app logo).
 */
export function BrandLogo({ size = 32, glow = false }: BrandLogoProps) {
  const gid = useId();
  const gradId = `brandGrad-${gid}`;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      fill="none"
      role="img"
      aria-label="MyShell"
      style={{
        display: "block",
        filter: glow ? "drop-shadow(0 0 10px var(--accent-primary-muted))" : "none",
      }}
    >
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style={{ stopColor: "var(--accent-primary-hover)" }} />
          <stop offset="0.5" style={{ stopColor: "var(--accent-primary)" }} />
          <stop offset="1" style={{ stopColor: "var(--accent-secondary)" }} />
        </linearGradient>
      </defs>
      {/* chevron `>` — vertex on the left, opens right */}
      <path
        d="M35 19 L16 32 L35 45"
        stroke={`url(#${gradId})`}
        strokeWidth="6.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {/* cursor `_` */}
      <rect
        x="37"
        y="42"
        width="13"
        height="5"
        rx="2.5"
        style={{ fill: "var(--accent-secondary)" }}
      />
    </svg>
  );
}
