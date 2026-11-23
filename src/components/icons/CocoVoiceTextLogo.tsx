import React from "react";
import CocoVoiceIcon from "./CocoVoiceIcon";

const WORDMARK = "Coco Voice";

const CocoVoiceTextLogo = ({
  width,
  className,
}: {
  width?: number;
  height?: number;
  className?: string;
}) => {
  return (
    <div
      className={`inline-flex items-center gap-2.5 select-none ${className ?? ""}`}
      style={width ? { maxWidth: width } : undefined}
    >
      <CocoVoiceIcon width={28} height={28} />
      <span
        style={{ fontFamily: "var(--font-display)" }}
        className="text-[22px] font-bold tracking-[-0.02em] text-text"
      >
        {WORDMARK}
      </span>
    </div>
  );
};

export default CocoVoiceTextLogo;
