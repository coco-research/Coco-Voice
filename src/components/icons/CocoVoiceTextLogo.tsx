import React from "react";

const CocoVoiceTextLogo = ({
  width,
  height,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  return (
    <svg
      width={width || 280}
      height={height || 48}
      className={className}
      viewBox="0 0 280 48"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id="coco-grad" x1="0" y1="0" x2="280" y2="0" gradientUnits="userSpaceOnUse">
          <stop stopColor="#4D7CFF" />
          <stop offset="0.5" stopColor="#2251FF" />
          <stop offset="1" stopColor="#4D7CFF" />
        </linearGradient>
      </defs>
      <text
        x="0"
        y="38"
        fontFamily="system-ui, -apple-system, sans-serif"
        fontSize="40"
        fontWeight="700"
        fill="url(#coco-grad)"
        letterSpacing="-0.5"
      >
        Coco Voice
      </text>
    </svg>
  );
};

export default CocoVoiceTextLogo;
