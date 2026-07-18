import React, { useState } from "react";
import { toast } from "sonner";
import type { CorrectionPair } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface CorrectionsProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Settings UI for the deterministic correction map. Each entry is a
 * case-insensitive, whole-word `from` → `to` replacement applied as the final
 * pass on the transcription output, after LLM post-processing and custom words.
 */
export const Corrections: React.FC<CorrectionsProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newFrom, setNewFrom] = useState("");
    const [newTo, setNewTo] = useState("");
    const corrections: CorrectionPair[] = getSetting("corrections") || [];

    const sanitize = (value: string) => value.replace(/[<>"']/g, "").trim();

    const handleAddPair = () => {
      const from = sanitize(newFrom);
      const to = sanitize(newTo);
      if (!from || from.length > 100 || to.length > 100) {
        return;
      }
      if (
        corrections.some((pair) => pair.from.toLowerCase() === from.toLowerCase())
      ) {
        toast.error(`A correction for "${from}" already exists`);
        return;
      }
      updateSetting("corrections", [...corrections, { from, to }]);
      setNewFrom("");
      setNewTo("");
    };

    const handleRemovePair = (fromToRemove: string) => {
      updateSetting(
        "corrections",
        corrections.filter((pair) => pair.from !== fromToRemove),
      );
    };

    const handleKeyPress = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddPair();
      }
    };

    const disabled = isUpdating("corrections");

    return (
      <>
        <SettingContainer
          title="Corrections"
          description="Replace specific words or phrases in the final output. Each rule is a case-insensitive, whole-word match applied after AI post-processing."
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-32"
              value={newFrom}
              onChange={(e) => setNewFrom(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder="From"
              variant="compact"
              disabled={disabled}
            />
            <span className="text-mid-gray">→</span>
            <Input
              type="text"
              className="max-w-32"
              value={newTo}
              onChange={(e) => setNewTo(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder="To"
              variant="compact"
              disabled={disabled}
            />
            <Button
              onClick={handleAddPair}
              disabled={!newFrom.trim() || newFrom.trim().length > 100 || disabled}
              variant="primary"
              size="md"
            >
              Add
            </Button>
          </div>
        </SettingContainer>
        {corrections.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-wrap gap-1`}
          >
            {corrections.map((pair) => (
              <Button
                key={pair.from}
                onClick={() => handleRemovePair(pair.from)}
                disabled={disabled}
                variant="secondary"
                size="sm"
                className="inline-flex items-center gap-1 cursor-pointer"
                aria-label={`Remove correction from ${pair.from} to ${pair.to}`}
              >
                <span>
                  {pair.from} → {pair.to || "(remove)"}
                </span>
                <svg
                  className="w-3 h-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M6 18L18 6M6 6l12 12"
                  />
                </svg>
              </Button>
            ))}
          </div>
        )}
      </>
    );
  },
);
