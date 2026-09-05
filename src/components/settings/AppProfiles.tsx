import React, { useState } from "react";
import { toast } from "sonner";
import type { AppProfile, CorrectionPair } from "@/bindings";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";

interface AppProfilesProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Settings UI for per-application post-processing profiles. Each profile maps
 * a frontmost application identifier (bundle ID on macOS, process name on
 * Windows/Linux) to optional overrides for prompt, provider, model, and
 * corrections. When the active app matches a profile, its overrides replace
 * the global defaults for that dictation session.
 */
export const AppProfiles: React.FC<AppProfilesProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newName, setNewName] = useState("");
    const [newIdentifier, setNewIdentifier] = useState("");
    const [detecting, setDetecting] = useState(false);

    const profiles: AppProfile[] = getSetting("app_profiles") || [];
    const disabled = isUpdating("app_profiles");

    const handleDetectActiveApp = async () => {
      setDetecting(true);
      try {
        const result = await commands.getActiveAppInfo();
        if (result.status === "ok" && result.data) {
          setNewIdentifier(result.data.app_name);
          toast.success(`Detected: ${result.data.app_name}`);
        } else {
          toast.error("Could not detect active application");
        }
      } catch {
        toast.error("Failed to detect active application");
      } finally {
        setDetecting(false);
      }
    };

    const handleAddProfile = () => {
      const name = newName.trim();
      const identifier = newIdentifier.trim();
      if (!name || !identifier) return;

      if (profiles.some((p) => p.app_identifier === identifier)) {
        toast.error(`A profile for "${identifier}" already exists`);
        return;
      }

      const profile: AppProfile = {
        id: crypto.randomUUID(),
        name,
        app_identifier: identifier,
        prompt_id: null,
        provider_id: null,
        model: null,
        corrections: [],
      };

      updateSetting("app_profiles", [...profiles, profile]);
      setNewName("");
      setNewIdentifier("");
    };

    const handleRemoveProfile = (id: string) => {
      updateSetting(
        "app_profiles",
        profiles.filter((p) => p.id !== id),
      );
    };

    const handleKeyPress = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddProfile();
      }
    };

    return (
      <>
        <SettingContainer
          title="Per-App Profiles"
          description="Override post-processing settings when specific applications are active. Match by app name or bundle identifier."
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex items-center gap-2">
            <Input
              type="text"
              className="max-w-40"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder="Profile name"
              variant="compact"
              disabled={disabled}
            />
            <Input
              type="text"
              className="max-w-48"
              value={newIdentifier}
              onChange={(e) => setNewIdentifier(e.target.value)}
              onKeyDown={handleKeyPress}
              placeholder="App identifier"
              variant="compact"
              disabled={disabled}
            />
            <Button
              onClick={handleDetectActiveApp}
              disabled={detecting || disabled}
              variant="secondary"
              size="md"
            >
              {detecting ? "Detecting…" : "Detect"}
            </Button>
            <Button
              onClick={handleAddProfile}
              disabled={!newName.trim() || !newIdentifier.trim() || disabled}
              variant="primary"
              size="md"
            >
              Add
            </Button>
          </div>
        </SettingContainer>
        {profiles.length > 0 && (
          <div
            className={`px-4 p-2 ${grouped ? "" : "rounded-lg border border-mid-gray/20"} flex flex-col gap-2`}
          >
            {profiles.map((profile) => (
              <div
                key={profile.id}
                className="flex items-center justify-between rounded-md bg-white/5 px-3 py-2"
              >
                <div className="flex flex-col">
                  <span className="text-sm font-medium">{profile.name}</span>
                  <span className="text-xs text-mid-gray">
                    {profile.app_identifier}
                  </span>
                </div>
                <Button
                  onClick={() => handleRemoveProfile(profile.id)}
                  disabled={disabled}
                  variant="secondary"
                  size="sm"
                  aria-label={`Remove profile ${profile.name}`}
                >
                  <svg
                    className="h-3 w-3"
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
              </div>
            ))}
          </div>
        )}
      </>
    );
  },
);
