import DOMPurify from "dompurify";
import { invoke } from "@tauri-apps/api/core";
import type { LunaAttachment } from "./types";

export const KGC_BASE = "https://kg-course.kwansei.ac.jp";
export const LUNA_BASE = "https://luna.kwansei.ac.jp";
export const KWIC_BASE = "https://kwic.kwansei.ac.jp";
const AUTO_LINK_RE = /(https?:\/\/[^\s<)\]]+|\/(?:lms|portal|cabinet|uniasv2)\/[^\s<)\]]+)/g;

export function readDetailParam(name: string): string {
  const search = new URLSearchParams(window.location.search);
  const fromSearch = search.get(name);
  if (fromSearch) return fromSearch;
  const rawHash = window.location.hash.startsWith("#") ? window.location.hash.slice(1) : window.location.hash;
  return new URLSearchParams(rawHash).get(name) || "";
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function autoLinkEscaped(value: string): string {
  return value.replace(AUTO_LINK_RE, (raw) =>
    `<a href="${raw}" target="_blank" rel="noopener">${raw}</a>`
  );
}

function autoLinkSanitizedHtml(value: string): string {
  if (typeof document === "undefined" || !value) return value;
  const template = document.createElement("template");
  template.innerHTML = value;
  const walker = document.createTreeWalker(template.content, NodeFilter.SHOW_TEXT);
  const nodes: Text[] = [];
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    if (node.parentElement?.closest("a, code, pre")) continue;
    if (AUTO_LINK_RE.test(node.data)) nodes.push(node);
    AUTO_LINK_RE.lastIndex = 0;
  }
  for (const node of nodes) {
    const replacement = document.createElement("span");
    replacement.innerHTML = autoLinkEscaped(escapeHtml(node.data));
    node.replaceWith(...Array.from(replacement.childNodes));
  }
  return template.innerHTML;
}

export function richText(value: string | undefined | null): string {
  const text = String(value || "").trim();
  if (!text) return "";
  const raw = /<\/?[a-z][\s\S]*>/i.test(text)
    ? text
    : autoLinkEscaped(escapeHtml(text))
        .replace(/\n{2,}/g, "</p><p>")
        .replace(/\n/g, "<br>");
  const wrapped = raw.startsWith("<") ? raw : `<p>${raw}</p>`;
  return autoLinkSanitizedHtml(DOMPurify.sanitize(wrapped, {
    ADD_ATTR: ["target", "rel", "class", "style"],
    FORBID_ATTR: ["onerror", "onload", "onclick", "onmouseover", "onfocus"],
  }));
}

function normalizeUrl(base: string, value: string): string {
  if (!value) return "";
  if (/^https?:\/\//i.test(value)) return value;
  return `${base}${value.startsWith("/") ? "" : "/"}${value}`;
}

export function normalizeLunaUrl(value: string): string {
  return normalizeUrl(LUNA_BASE, value);
}

export function normalizeKgcUrl(value: string): string {
  return normalizeUrl(KGC_BASE, value);
}

export function normalizeKwicUrl(value: string): string {
  return normalizeUrl(KWIC_BASE, value);
}

export function attachmentName(attachment: LunaAttachment): string {
  return attachment.name || attachment.file_name || "添付ファイル";
}

/**
 * Strip internal platform names (Luna / KWIC / KGC / KGU) from a message before
 * showing it to the user. Detection markers (expiredMarkers, isLunaAuthError)
 * keep matching the raw backend names — only the displayed text is sanitized.
 */
export function displaySafeMessage(message: string): string {
  return String(message ?? "")
    .replace(/(?:Luna|LUNA|KWIC|KGC|KGU)(?:ポータル)?にログインしてください/g, "ログインしてください")
    .replace(/(?:Luna|LUNA|KWIC|KGC|KGU)(?:ポータル)?セッション/g, "セッション")
    .replace(/(?:Luna|LUNA|KWIC|KGC|KGU)(?:ポータル|システム)?/g, "")
    .replace(/\s{2,}/g, " ")
    .trim();
}

export function isLunaAuthError(error: unknown): boolean {
  const message = String(error instanceof Error ? error.message : error || "");
  return message.includes("Lunaセッションが期限切れです")
    || message.includes("Lunaにログインしてください");
}

export function isLunaTransientDetailError(error: unknown): boolean {
  const message = String(error instanceof Error ? error.message : error || "");
  return message.includes("読込が一時的に不安定")
    || message.includes("初回読込が安定していません");
}

export async function invokeLunaDetailWithRetry<T>(
  command: string,
  payload: Record<string, unknown>,
  attempt = 1,
): Promise<T> {
  try {
    return await invoke<T>(command, payload);
  } catch (error) {
    if (isLunaAuthError(error)) {
      const recovered = attempt < 2
        && await invoke<boolean>("sync_session", { service: "luna" }).catch(() => false);
      if (recovered) return invokeLunaDetailWithRetry<T>(command, payload, attempt + 1);
      throw error;
    }
    if (attempt >= 2 || !isLunaTransientDetailError(error)) throw error;
    await new Promise((resolve) => window.setTimeout(resolve, 500 * attempt));
    return invokeLunaDetailWithRetry<T>(command, payload, attempt + 1);
  }
}
