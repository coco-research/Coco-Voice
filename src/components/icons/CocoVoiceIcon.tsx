import React from "react";

const CocoVoiceIcon = ({
  width,
  height,
}: {
  width?: number | string;
  height?: number | string;
}) => (
  <svg
    width={width || 32}
    height={height || 32}
    viewBox="0 0 64 64"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <defs>
      <radialGradient id="coco-orb-core" cx="38%" cy="32%" r="65%">
        <stop offset="0%" stopColor="#A9C4FF" />
        <stop offset="52%" stopColor="#5B8DFF" />
        <stop offset="100%" stopColor="#1D4ED8" />
      </radialGradient>
    </defs>
    <circle cx="32" cy="32" r="26" fill="url(#coco-orb-core)" />
    <circle cx="25" cy="23" r="8" fill="#EEF2FB" fillOpacity="0.38" />
    <circle
      cx="32"
      cy="32"
      r="26"
      stroke="#A9C4FF"
      strokeOpacity="0.45"
      strokeWidth="1.5"
    />
  </svg>
);

export default CocoVoiceIcon;
