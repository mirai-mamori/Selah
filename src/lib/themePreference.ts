import { emit } from "@tauri-apps/api/event";
import { setAppTheme } from "./system";

export type ThemePreference = "light" | "dark" | "auto";
export type EffectiveTheme = "light" | "dark";

export const AUTO_LIGHT_START_HOUR = 7;
export const AUTO_DARK_START_HOUR = 19;

export function resolveThemePreference(
  preference: ThemePreference,
  date = new Date()
): EffectiveTheme {
  if (preference === "light" || preference === "dark") return preference;
  const hour = date.getHours();
  return hour >= AUTO_LIGHT_START_HOUR && hour < AUTO_DARK_START_HOUR ? "light" : "dark";
}

export function normalizeEffectiveTheme(value: unknown): EffectiveTheme | "" {
  if (value === "light" || value === "dark") return value;
  if (value === "auto" || value === "system") return resolveThemePreference("auto");
  return "";
}

export function readThemePreference(): ThemePreference {
  try {
    const saved = localStorage.getItem("selah-theme");
    if (saved === "light" || saved === "dark" || saved === "auto") return saved;
    // Older builds used "system" for the unset/default state.
    if (saved === "system") return "auto";
  } catch {}
  return "auto";
}

function applyEffectiveTheme(theme: EffectiveTheme): void {
  document.documentElement.setAttribute("data-theme", theme);
  void setAppTheme(theme).catch(() => {});
}

export function initializeThemePreference(): ThemePreference {
  const preference = readThemePreference();
  applyEffectiveTheme(resolveThemePreference(preference));
  return preference;
}

export function applyThemePreference(preference: ThemePreference): EffectiveTheme {
  const effective = resolveThemePreference(preference);
  try {
    localStorage.setItem("selah-theme", preference);
  } catch {}
  applyEffectiveTheme(effective);
  void emit("theme-changed", effective).catch(() => {});
  return effective;
}
