import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { AppProfile, CorrectionPair } from "@/bindings";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

function isCocoVoiceApp(appName: string, processPath: string): boolean {
  const fileName = processPath.split(/[/\\]/).pop() ?? "";
  const stem = fileName.replace(/\.[^.]+$/, "");
  const own = (value: string) => {
    const normalized = value.trim().toLowerCase();
    return normalized === "coco voice" || normalized === "coco-voice";
  };
  return own(appName) || own(stem);
}

interface AppProfilesProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Settings UI for per-application post-processing profiles. Each profile maps
 * the frontmost application's name or process name to optional overrides for
 * prompt, provider, model, and corrections. When the active app matches a
 * profile, its overrides replace the global defaults for that dictation.
 */
export const AppProfiles: React.FC<AppProfilesProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const [newName, setNewName] = useState("");
    const [newIdentifier, setNewIdentifier] = useState("");
    const [detecting, setDetecting] = useState(false);
    const [detectHint, setDetectHint] = useState<string | null>(null);
    const mounted = useRef(true);
    useEffect(() => {
      mounted.current = true;
      return () => {
        mounted.current = false;
      };
    }, []);
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

    const readProfiles = (): AppProfile[] =>
      useSettingsStore.getState().settings?.app_profiles ?? [];

    const updateProfile = (id: string, patch: Partial<AppProfile>) => {
      updateSetting(
        "app_profiles",
        readProfiles().map((p) => (p.id === id ? { ...p, ...patch } : p)),
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

    const handleAddCorrection = (profileId: string) => {
      const profile = readProfiles().find((p) => p.id === profileId);
      if (!profile) return;
      const draft = getCorrectionDraft(profile.id);
      const from = sanitize(draft.from);
      const to = sanitize(draft.to);
      if (!from) {
        toast.error(
          "That correction is empty after removing quotes and brackets",
        );
        return;
      }
      if (from.length > 100 || to.length > 100) {
        toast.error("Corrections must be 100 characters or fewer");
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

    const handleRemoveCorrection = (profileId: string, from: string) => {
      const profile = readProfiles().find((p) => p.id === profileId);
      if (!profile) return;
      updateProfile(profile.id, {
        corrections: (profile.corrections ?? []).filter(
          (pair) => pair.from !== from,
        ),
      });
    };

    const handleDetectActiveApp = async () => {
      setDetecting(true);
      setDetectHint(null);
      try {
        for (const seconds of [3, 2, 1]) {
          if (!mounted.current) return;
          setDetectHint(t("settings.appProfiles.detectCountdown", { seconds }));
          await new Promise((resolve) => setTimeout(resolve, 1000));
        }
        if (!mounted.current) return;
        const result = await commands.getActiveAppInfo();
        if (!mounted.current) return;
        if (result.status === "ok" && result.data) {
          if (isCocoVoiceApp(result.data.app_name, result.data.process_path)) {
            toast.error(t("settings.appProfiles.detectOwnApp"));
            return;
          }
          setNewIdentifier(result.data.app_name);
          toast.success(`Detected: ${result.data.app_name}`);
        } else {
          toast.error("Could not detect active application");
        }
      } catch {
        toast.error("Failed to detect active application");
      } finally {
        if (mounted.current) {
          setDetecting(false);
          setDetectHint(null);
        }
      }
    };

    const handleAddProfile = () => {
      const name = newName.trim();
      const identifier = newIdentifier.trim();
      if (!name || !identifier) return;

      const current = readProfiles();
      if (
        current.some(
          (p) => p.app_identifier.toLowerCase() === identifier.toLowerCase(),
        )
      ) {
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

      updateSetting("app_profiles", [...current, profile]);
      setNewName("");
      setNewIdentifier("");
    };

    const handleRemoveProfile = (id: string) => {
      updateSetting(
        "app_profiles",
        readProfiles().filter((p) => p.id !== id),
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
          description="Override post-processing settings when specific applications are active. Matched against the active app's name or process name."
          descriptionMode={descriptionMode}
          grouped={grouped}
        >
          <div className="flex flex-col gap-2">
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
                {t("settings.appProfiles.add")}
              </Button>
            </div>
            {detectHint ? (
              <p className="text-xs text-mid-gray">{detectHint}</p>
            ) : null}
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
                      key={`${profile.id}:${profile.model ?? ""}`}
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
                          handleAddCorrection(profile.id);
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
                          handleAddCorrection(profile.id);
                        }
                      }}
                      placeholder="To"
                      variant="compact"
                      disabled={disabled}
                    />
                    <Button
                      onClick={() => handleAddCorrection(profile.id)}
                      disabled={!draft.from.trim() || disabled}
                      variant="secondary"
                      size="sm"
                    >
                      {t("settings.appProfiles.addCorrection")}
                    </Button>
                  </div>

                  {corrections.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {corrections.map((pair) => (
                        <Button
                          key={pair.from}
                          onClick={() =>
                            handleRemoveCorrection(profile.id, pair.from)
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
