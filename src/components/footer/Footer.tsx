import React, { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";

import ModelSelector from "../model-selector";
import UpdateChecker from "../update-checker";

export async function resolveFooterVersion(
  loader: () => Promise<string> = getVersion,
): Promise<string> {
  try {
    const appVersion = await loader();
    return appVersion.trim();
  } catch (error) {
    console.error("Failed to get app version:", error);
    return "";
  }
}

const Footer: React.FC = () => {
  const [version, setVersion] = useState("");

  useEffect(() => {
    let cancelled = false;
    void resolveFooterVersion().then((appVersion) => {
      if (!cancelled) {
        setVersion(appVersion);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="w-full border-t border-mid-gray/20 pt-3">
      <div className="flex justify-between items-center text-xs px-4 pb-3 text-text/60">
        <div className="flex items-center gap-4">
          <ModelSelector />
        </div>

        {/* Update Status */}
        <div className="flex items-center gap-1">
          <UpdateChecker />
          {version ? (
            <>
              <span>•</span>
              {/* eslint-disable-next-line i18next/no-literal-string */}
              <span>v{version}</span>
            </>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export default Footer;
