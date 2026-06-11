import { invoke } from "@tauri-apps/api/core";

function normalizeTheme(value: unknown): "dark" | "light" | "" {
  return value === "dark" || value === "light" ? value : "";
}

export function applyAuxiliaryTheme(value: unknown): void {
  const theme = normalizeTheme(value);
  if (theme) {
    document.documentElement.setAttribute("data-theme", theme);
    document.body.setAttribute("data-theme", theme);
  } else {
    document.documentElement.removeAttribute("data-theme");
    document.body.removeAttribute("data-theme");
  }
}

export async function syncAuxiliaryTheme(): Promise<void> {
  try {
    const stored = localStorage.getItem("selah-theme") || "";
    const appTheme = await invoke<string>("get_app_theme");
    applyAuxiliaryTheme(normalizeTheme(appTheme) || stored);
  } catch {
    try {
      applyAuxiliaryTheme(localStorage.getItem("selah-theme") || "");
    } catch {}
  }
}
