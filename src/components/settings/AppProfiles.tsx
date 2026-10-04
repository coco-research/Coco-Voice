import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { AppProfile, CorrectionPair } from "@/bindings";
import { commands } from "@/bindings";
import { useOsType } from "../../hooks/useOsType";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import { Input } from "../ui/Input";
import { Button } from "../ui/Button";
import { Dropdown, type DropdownOption } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";

// Caps in src-tauri/src/shortcut/mod.rs (normalize_app_profiles). The backend
// cuts longer values, so the UI must not let them be entered. maxLength counts
// UTF-16 units, which is never looser than the backend's per-character cap.
const MAX_APP_PROFILES = 64;
const MAX_PROFILE_NAME = 120;
const MAX_APP_IDENTIFIER = 256;
const MAX_PROFILE_MODEL = 256;
const MAX_PROFILE_CORRECTIONS = 50;
const MAX_CORRECTION_TEXT = 100;

const COCO_VOICE_NAMES = new Set([
  "coco-voice",
  "cocovoice",
  "coco_voice",
  "coco voice",
]);

function isCocoVoiceName(value: string): boolean {
  return COCO_VOICE_NAMES.has(value.trim().toLowerCase());
}

/**
 * Mirrors Rust's Path::file_stem, which the backend matches profiles against:
 * the last path component with only its final extension removed. "." parts are
 * skipped, a trailing ".." has no stem, and a leading dot is not an extension.
 * A backslash separates components on Windows only.
 */
function processFileStem(processPath: string, windows: boolean): string {
  const name =
    processPath
      .split(windows ? /[/\\]/ : "/")
      .filter((part) => part !== "" && part !== ".")
      .pop() ?? "";
  if (name === "..") return "";
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

// For values set in code (Detect), where maxLength does not apply: the backend
// counts characters, not UTF-16 units.
const charCount = (value: string) => Array.from(value).length;

const EMPTY_DRAFT = { from: "", to: "" };

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
    const { getSetting, isUpdating } = useSettings();
    const updateAppProfiles = useSettingsStore(
      (state) => state.updateAppProfiles,
    );
    const windows = useOsType() === "windows";
    const [newName, setNewName] = useState("");
    const [newIdentifier, setNewIdentifier] = useState("");
    const [detecting, setDetecting] = useState(false);
    const [detectHint, setDetectHint] = useState<string | null>(null);
    const detectGeneration = useRef(0);
    const countdownTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
    const countdownResolve = useRef<(() => void) | null>(null);

    const clearCountdown = () => {
      if (countdownTimeout.current !== null) {
        clearTimeout(countdownTimeout.current);
        countdownTimeout.current = null;
      }
      const resolve = countdownResolve.current;
      countdownResolve.current = null;
      resolve?.();
    };

    useEffect(() => {
      return () => {
        detectGeneration.current += 1;
        clearCountdown();
      };
    }, []);
    const [correctionDrafts, setCorrectionDrafts] = useState<
      Record<string, { from: string; to: string }>
    >({});

    const profiles: AppProfile[] = getSetting("app_profiles") || [];
    const disabled = isUpdating("app_profiles");
    const atProfileLimit = profiles.length >= MAX_APP_PROFILES;

    const globalDefault = t(
      "settings.postProcessing.appProfiles.globalDefault",
    );
    const promptOptions: DropdownOption[] = [
      { value: "", label: globalDefault },
      ...(getSetting("post_process_prompts") || []).map((prompt) => ({
        value: prompt.id,
        label: prompt.name,
      })),
    ];
    const providerOptions: DropdownOption[] = [
      { value: "", label: globalDefault },
      ...(getSetting("post_process_providers") || []).map((provider) => ({
        value: provider.id,
        label: provider.label,
      })),
    ];
    const providerModels = getSetting("post_process_models");

    // Edits one profile as it is when the edit runs, not as it was when the
    // control rendered, so overlapping edits build on each other. `edit`
    // returns the fields to change, or null for no change; a patch that matches
    // the profile already is skipped so nothing is saved for it.
    const editProfile = (
      id: string,
      edit: (profile: AppProfile) => Partial<AppProfile> | null,
    ) =>
      updateAppProfiles((current) => {
        const profile = current.find((p) => p.id === id);
        const patch = profile && edit(profile);
        if (!profile || !patch) return null;
        const unchanged = (Object.keys(patch) as (keyof AppProfile)[]).every(
          (key) => (profile[key] ?? null) === (patch[key] ?? null),
        );
        return unchanged
          ? null
          : current.map((p) => (p.id === id ? { ...p, ...patch } : p));
      });

    const getCorrectionDraft = (id: string) =>
      correctionDrafts[id] ?? EMPTY_DRAFT;

    const setCorrectionDraft = (
      id: string,
      patch: Partial<{ from: string; to: string }>,
    ) => {
      setCorrectionDrafts((prev) => ({
        ...prev,
        [id]: { ...(prev[id] ?? EMPTY_DRAFT), ...patch },
      }));
    };

    const handleAddCorrection = async (profileId: string) => {
      const draft = getCorrectionDraft(profileId);
      const from = draft.from.trim();
      const to = draft.to.trim();
      if (!from) return;
      const saved = await editProfile(profileId, (profile) => {
        const corrections = profile.corrections ?? [];
        if (corrections.length >= MAX_PROFILE_CORRECTIONS) {
          toast.error(
            t("settings.postProcessing.appProfiles.correctionLimit", {
              max: MAX_PROFILE_CORRECTIONS,
            }),
          );
          return null;
        }
        if (
          corrections.some(
            (pair) => pair.from.toLowerCase() === from.toLowerCase(),
          )
        ) {
          toast.error(
            t("settings.postProcessing.appProfiles.correctionExists", { from }),
          );
          return null;
        }
        return { corrections: [...corrections, { from, to }] };
      });
      if (saved) setCorrectionDraft(profileId, EMPTY_DRAFT);
    };

    const handleRemoveCorrection = (profileId: string, from: string) =>
      editProfile(profileId, (profile) => ({
        corrections: (profile.corrections ?? []).filter(
          (pair) => pair.from !== from,
        ),
      }));

    const waitForCountdownTick = () =>
      new Promise<void>((resolve) => {
        countdownResolve.current = resolve;
        countdownTimeout.current = setTimeout(() => {
          countdownTimeout.current = null;
          countdownResolve.current = null;
          resolve();
        }, 1000);
      });

    const handleDetectActiveApp = async () => {
      detectGeneration.current += 1;
      clearCountdown();
      const generation = detectGeneration.current;
      setDetecting(true);
      setDetectHint(null);
      try {
        for (const seconds of [3, 2, 1]) {
          // A cancel resolves the pending wait; this check then ends the loop
          // before another wait starts.
          if (detectGeneration.current !== generation) return;
          setDetectHint(
            t("settings.postProcessing.appProfiles.detectCountdown", {
              seconds,
            }),
          );
          await waitForCountdownTick();
        }
        if (detectGeneration.current !== generation) return;
        const result = await commands.getActiveAppInfo();
        if (detectGeneration.current !== generation) return;
        if (result.status === "ok" && result.data) {
          const stem = processFileStem(
            result.data.process_path,
            windows,
          ).trim();
          const appName = result.data.app_name.trim();
          if (isCocoVoiceName(stem) || isCocoVoiceName(appName)) {
            toast.error(t("settings.postProcessing.appProfiles.detectOwnApp"));
            return;
          }
          // The backend cuts identifiers at MAX_APP_IDENTIFIER characters, and a
          // cut identifier never equals the full name, so only offer one that fits.
          const identifier = [stem, appName].find(
            (name) => name && charCount(name) <= MAX_APP_IDENTIFIER,
          );
          if (!identifier) {
            toast.error(t("settings.postProcessing.appProfiles.detectFailed"));
            return;
          }
          setNewIdentifier(identifier);
          if (appName && identifier === stem && appName !== stem) {
            toast.success(
              t("settings.postProcessing.appProfiles.detectedWithStem", {
                name: appName,
                stem,
              }),
            );
          } else {
            toast.success(
              t("settings.postProcessing.appProfiles.detected", {
                name: identifier,
              }),
            );
          }
        } else {
          toast.error(t("settings.postProcessing.appProfiles.detectFailed"));
        }
      } catch {
        if (detectGeneration.current === generation) {
          toast.error(t("settings.postProcessing.appProfiles.detectFailed"));
        }
      } finally {
        if (detectGeneration.current === generation) {
          setDetecting(false);
          setDetectHint(null);
        }
      }
    };

    const handleAddProfile = async () => {
      const name = newName.trim();
      const identifier = newIdentifier.trim();
      if (!name || !identifier) return;

      const saved = await updateAppProfiles((current) => {
        if (current.length >= MAX_APP_PROFILES) {
          toast.error(
            t("settings.postProcessing.appProfiles.profileLimit", {
              max: MAX_APP_PROFILES,
            }),
          );
          return null;
        }
        if (
          current.some(
            (p) => p.app_identifier.toLowerCase() === identifier.toLowerCase(),
          )
        ) {
          toast.error(
            t("settings.postProcessing.appProfiles.duplicateProfile", {
              identifier,
            }),
          );
          return null;
        }
        return [
          ...current,
          {
            id: crypto.randomUUID(),
            name,
            app_identifier: identifier,
            prompt_id: null,
            provider_id: null,
            model: null,
            corrections: [],
          },
        ];
      });
      if (saved) {
        setNewName("");
        setNewIdentifier("");
      }
    };

    const handleRemoveProfile = (id: string) =>
      updateAppProfiles((current) => {
        const next = current.filter((p) => p.id !== id);
        return next.length === current.length ? null : next;
      });

    const handleKeyPress = (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        handleAddProfile();
      }
    };

    return (
      <>
        <SettingContainer
          title={t("settings.postProcessing.appProfiles.addProfile")}
          description={t("settings.postProcessing.appProfiles.description")}
          descriptionMode={descriptionMode}
          grouped={grouped}
          layout="stacked"
        >
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <Input
                type="text"
                className="min-w-[120px] max-w-40 flex-1"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                onKeyDown={handleKeyPress}
                placeholder={t(
                  "settings.postProcessing.appProfiles.namePlaceholder",
                )}
                variant="compact"
                disabled={disabled}
                maxLength={MAX_PROFILE_NAME}
              />
              <Input
                type="text"
                className="min-w-[120px] max-w-48 flex-1"
                value={newIdentifier}
                onChange={(e) => setNewIdentifier(e.target.value)}
                onKeyDown={handleKeyPress}
                placeholder={t(
                  "settings.postProcessing.appProfiles.identifierPlaceholder",
                )}
                variant="compact"
                disabled={disabled}
                maxLength={MAX_APP_IDENTIFIER}
              />
              <Button
                onClick={handleDetectActiveApp}
                disabled={detecting || disabled}
                variant="secondary"
                size="md"
              >
                {t("settings.postProcessing.appProfiles.detect")}
              </Button>
              <Button
                onClick={handleAddProfile}
                disabled={
                  !newName.trim() ||
                  !newIdentifier.trim() ||
                  disabled ||
                  atProfileLimit
                }
                variant="primary"
                size="md"
              >
                {t("settings.advanced.customWords.add")}
              </Button>
            </div>
            {detectHint ? (
              <p className="text-xs text-mid-gray">{detectHint}</p>
            ) : null}
            {atProfileLimit ? (
              <p className="text-xs text-mid-gray">
                {t("settings.postProcessing.appProfiles.profileLimit", {
                  max: MAX_APP_PROFILES,
                })}
              </p>
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
              const atCorrectionLimit =
                corrections.length >= MAX_PROFILE_CORRECTIONS;
              // The backend only switches to the profile's provider when a model
              // is set for it, here or in the global provider settings.
              const providerModel = profile.provider_id
                ? (providerModels?.[profile.provider_id]?.trim() ?? "")
                : "";
              const needsModel =
                !!profile.provider_id && !providerModel && !profile.model;
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
                      aria-label={t("settings.advanced.customWords.remove", {
                        word: profile.name,
                      })}
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
                        editProfile(profile.id, () => ({
                          prompt_id: value || null,
                        }))
                      }
                      disabled={disabled}
                      className="min-w-[180px] flex-1"
                    />
                    <Dropdown
                      options={providerOptions}
                      selectedValue={profile.provider_id ?? ""}
                      onSelect={(value) =>
                        // A model only applies with the provider the profile
                        // names, so clearing the provider clears the model.
                        editProfile(profile.id, () =>
                          value
                            ? { provider_id: value }
                            : { provider_id: null, model: null },
                        )
                      }
                      disabled={disabled}
                      className="min-w-[180px] flex-1"
                    />
                    {profile.provider_id ? (
                      <Input
                        key={`${profile.id}:${profile.model ?? ""}`}
                        type="text"
                        className="min-w-[180px] flex-1"
                        defaultValue={profile.model ?? ""}
                        onBlur={(e) => {
                          const model = e.target.value.trim();
                          editProfile(profile.id, () => ({
                            model: model || null,
                          }));
                        }}
                        placeholder={
                          providerModel ||
                          t(
                            "settings.postProcessing.appProfiles.modelPlaceholder",
                          )
                        }
                        variant="compact"
                        disabled={disabled}
                        maxLength={MAX_PROFILE_MODEL}
                      />
                    ) : null}
                    {needsModel ? (
                      <p className="w-full text-xs text-mid-gray">
                        {t("settings.postProcessing.appProfiles.modelRequired")}
                      </p>
                    ) : null}
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
                      placeholder={t(
                        "settings.postProcessing.appProfiles.fromPlaceholder",
                      )}
                      variant="compact"
                      disabled={disabled}
                      maxLength={MAX_CORRECTION_TEXT}
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
                      placeholder={t(
                        "settings.postProcessing.appProfiles.toPlaceholder",
                      )}
                      variant="compact"
                      disabled={disabled}
                      maxLength={MAX_CORRECTION_TEXT}
                    />
                    <Button
                      onClick={() => handleAddCorrection(profile.id)}
                      disabled={
                        !draft.from.trim() || disabled || atCorrectionLimit
                      }
                      variant="secondary"
                      size="sm"
                    >
                      {t("settings.postProcessing.appProfiles.addCorrection")}
                    </Button>
                    {atCorrectionLimit ? (
                      <p className="w-full text-xs text-mid-gray">
                        {t(
                          "settings.postProcessing.appProfiles.correctionLimit",
                          {
                            max: MAX_PROFILE_CORRECTIONS,
                          },
                        )}
                      </p>
                    ) : null}
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
                          aria-label={t(
                            "settings.advanced.customWords.remove",
                            {
                              word: `${pair.from} → ${pair.to}`,
                            },
                          )}
                        >
                          <span>
                            {pair.from} →{" "}
                            {pair.to ||
                              t(
                                "settings.postProcessing.appProfiles.removeText",
                              )}
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
