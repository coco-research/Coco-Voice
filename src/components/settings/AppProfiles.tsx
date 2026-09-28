import React, { useState } from "react";
import { toast } from "sonner";
import type { AppProfile, CorrectionPair } from "@/bindings";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
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
    const [correctionDrafts, setCorrectionDrafts] = useState<
      Record<string, { from: string; to: string }>
    >({});

    const profiles: AppProfile[] = getSetting("app_profiles") || [];
    const disabled = isUpdating("app_profiles");

    const promptOptions: DropdownOption[] = [
      { value: "", label: "Global default" },
      ...(getSetting("post_process_prompts") || []).map((prompt) => ({
        value: prompt.id,
        label: prompt.name,
      })),
    ];
    const providerOptions: DropdownOption[] = [
      { value: "", label: "Global default" },
      ...(getSetting("post_process_providers") || []).map((provider) => ({
        value: provider.id,
        label: provider.label,
      })),
    ];

    const sanitize = (value: string) => value.replace(/[<>"']/g, "").trim();

    const updateProfile = (id: string, patch: Partial<AppProfile>) => {
      updateSetting(
        "app_profiles",
        profiles.map((p) => (p.id === id ? { ...p, ...patch } : p)),
      );
    };

    const getCorrectionDraft = (id: string) =>
      correctionDrafts[id] ?? { from: "", to: "" };

    const setCorrectionDraft = (
      id: string,
      patch: Partial<{ from: string; to: string }>,
    ) => {
      setCorrectionDrafts((prev) => ({
        ...prev,
        [id]: { ...getCorrectionDraft(id), ...patch },
      }));
    };

    const handleAddCorrection = (profile: AppProfile) => {
      const draft = getCorrectionDraft(profile.id);
      const from = sanitize(draft.from);
      const to = sanitize(draft.to);
      if (!from || from.length > 100 || to.length > 100) {
        return;
      }
      const corrections = profile.corrections ?? [];
      if (
        corrections.some(
          (pair) => pair.from.toLowerCase() === from.toLowerCase(),
        )
      ) {
        toast.error(`A correction for "${from}" already exists`);
        return;
      }
      updateProfile(profile.id, {
        corrections: [...corrections, { from, to }],
      });
      setCorrectionDraft(profile.id, { from: "", to: "" });
    };

    const handleRemoveCorrection = (profile: AppProfile, from: string) => {
      updateProfile(profile.id, {
        corrections: (profile.corrections ?? []).filter(
          (pair) => pair.from !== from,
        ),
      });
    };

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
            {profiles.map((profile) => {
              const draft = getCorrectionDraft(profile.id);
              const corrections: CorrectionPair[] = profile.corrections ?? [];
              return (
                <div
                  key={profile.id}
                  className="flex flex-col gap-2 rounded-md bg-white/5 px-3 py-2"
                >
                  <div className="flex items-center justify-between">
                    <div className="flex flex-col">
                      <span className="text-sm font-medium">
                        {profile.name}
                      </span>
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

                  <div className="flex flex-wrap items-center gap-2">
                    <Dropdown
                      options={promptOptions}
                      selectedValue={profile.prompt_id ?? ""}
                      onSelect={(value) =>
                        updateProfile(profile.id, {
                          prompt_id: value || null,
                        })
                      }
                      disabled={disabled}
                      className="min-w-[180px] flex-1"
                    />
                    <Dropdown
                      options={providerOptions}
                      selectedValue={profile.provider_id ?? ""}
                      onSelect={(value) =>
                        updateProfile(profile.id, {
                          provider_id: value || null,
                        })
                      }
                      disabled={disabled}
                      className="min-w-[180px] flex-1"
                    />
                    <Input
                      type="text"
                      className="min-w-[180px] flex-1"
                      defaultValue={profile.model ?? ""}
                      onBlur={(e) =>
                        updateProfile(profile.id, {
                          model: e.target.value.trim() || null,
                        })
                      }
                      placeholder="Model (global default)"
                      variant="compact"
                      disabled={disabled}
                    />
                  </div>

                  <div className="flex flex-wrap items-center gap-2">
                    <Input
                      type="text"
                      className="max-w-32"
                      value={draft.from}
                      onChange={(e) =>
                        setCorrectionDraft(profile.id, {
                          from: e.target.value,
                        })
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          handleAddCorrection(profile);
                        }
                      }}
                      placeholder="From"
                      variant="compact"
                      disabled={disabled}
                    />
                    <span className="text-mid-gray">→</span>
                    <Input
                      type="text"
                      className="max-w-32"
                      value={draft.to}
                      onChange={(e) =>
                        setCorrectionDraft(profile.id, { to: e.target.value })
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          handleAddCorrection(profile);
                        }
                      }}
                      placeholder="To"
                      variant="compact"
                      disabled={disabled}
                    />
                    <Button
                      onClick={() => handleAddCorrection(profile)}
                      disabled={!draft.from.trim() || disabled}
                      variant="secondary"
                      size="sm"
                    >
                      Add correction
                    </Button>
                  </div>

                  {corrections.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {corrections.map((pair) => (
                        <Button
                          key={pair.from}
                          onClick={() =>
                            handleRemoveCorrection(profile, pair.from)
                          }
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
                </div>
              );
            })}
          </div>
        )}
      </>
    );
  },
);
