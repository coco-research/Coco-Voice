export type ShellGroup = "top" | "meetings" | "voice" | "shared";
export type RouteDomain = "home" | "meetings" | "voice" | "shared";
const route = <const Id extends string>(
  id: Id,
  group: ShellGroup,
  domain: RouteDomain,
  labelKey: string,
) => ({ id, group, domain, labelKey });

export const SHELL_ROUTES = [
  route("home", "top", "home", "sidebar.home"),
  route("meetings-record", "meetings", "meetings", "sidebar.recordMeeting"),
  route("meetings-history", "meetings", "meetings", "sidebar.meetingHistory"),
  route("meetings-ask", "meetings", "meetings", "sidebar.askMeetings"),
  route(
    "meetings-folders",
    "meetings",
    "meetings",
    "sidebar.foldersAndTemplates",
  ),
  route("voice-history", "voice", "voice", "sidebar.dictationHistory"),
  route("voice-profiles", "voice", "voice", "sidebar.perAppProfiles"),
  route("voice-words", "voice", "voice", "sidebar.wordsAndCleanup"),
  route("models", "shared", "shared", "sidebar.models"),
  route("settings", "shared", "shared", "sidebar.settings"),
  route("about", "shared", "shared", "sidebar.about"),
] as const;

export type ShellSectionId = (typeof SHELL_ROUTES)[number]["id"] | "debug";
export const isMeetingSection = (section: ShellSectionId) =>
  section.startsWith("meetings-");
export function normalizeShellSection(
  section: ShellSectionId,
  meetingsVisible: boolean,
  debugEnabled: boolean,
): ShellSectionId {
  if (isMeetingSection(section) && !meetingsVisible) return "home";
  if (section === "debug" && !debugEnabled) return "settings";
  return section;
}
