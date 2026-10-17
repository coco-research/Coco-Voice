import React from "react";

/**
 * The mark that opens a "cleanup did not run" note, so its kind never rests on
 * colour alone: a filled amber diamond when the user can fix the cause, a quiet
 * open circle when it concerns this take only. The colours are the
 * `--color-needs-you*` and `--color-quiet` tokens of `styles/theme.css`.
 */
export const CleanupMark: React.FC<{ needsAction: boolean }> = ({
  needsAction,
}) => (
  <svg
    viewBox="0 0 12 12"
    width="1em"
    height="1em"
    aria-hidden="true"
    style={{
      flex: "none",
      strokeWidth: 1.4,
      strokeLinejoin: "round",
      fill: needsAction ? "var(--color-needs-you-fill)" : "none",
      stroke: needsAction ? "var(--color-needs-you)" : "var(--color-quiet)",
    }}
  >
    {needsAction ? (
      <path d="M6 1.2 10.8 6 6 10.8 1.2 6Z" />
    ) : (
      <circle cx="6" cy="6" r="4.6" />
    )}
  </svg>
);
