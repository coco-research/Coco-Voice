import React from "react";

const CocoVoiceIcon = ({
  width,
  height,
}: {
  width?: number | string;
  height?: number | string;
}) => (
  <svg
    width={width || 48}
    height={height || 48}
    viewBox="0 0 48 48"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
  >
    {/* Microphone body */}
    <rect
      x="18"
      y="4"
      width="12"
      height="20"
      rx="6"
      className="fill-text"
      strokeWidth="2"
      stroke="currentColor"
    />
    {/* Microphone grill lines */}
    <line x1="21" y1="10" x2="21" y2="16" stroke="currentColor" strokeWidth="1" strokeLinecap="round" />
    <line x1="24" y1="10" x2="24" y2="16" stroke="currentColor" strokeWidth="1" strokeLinecap="round" />
    <line x1="27" y1="10" x2="27" y2="16" stroke="currentColor" strokeWidth="1" strokeLinecap="round" />
    {/* Mic base curve */}
    <path d="M16 30 Q24 40 32 30" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    {/* Stand line */}
    <line x1="24" y1="30" x2="24" y2="42" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    {/* Base */}
    <line x1="18" y1="42" x2="30" y2="42" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    {/* Sound waves (right side) */}
    <path d="M36 14 Q42 22 36 30" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" opacity="0.7" />
    <path d="M40 10 Q48 22 40 34" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" opacity="0.4" />
  </svg>
);

export default CocoVoiceIcon;
