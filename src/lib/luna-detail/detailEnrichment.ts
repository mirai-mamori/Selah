import { invoke } from "@tauri-apps/api/core";
import { KGC_BASE, KWIC_BASE, LUNA_BASE } from "./runtime";
import type { LunaContentItem, LunaCourseContents, LunaDetailPage, MetaPair } from "./types";

interface LinkContext {
  courseName?: string;
  idnumber?: string;
}

interface ScheduleEntry {
  name?: string;
  detail_path?: string;
}

interface ScheduleSnapshot {
  raw?: {
    kgc_entries_current?: ScheduleEntry[];
    kgc_entries_next?: ScheduleEntry[];
  };
}

interface CachedTodo {
  url?: string;
  content_name?: string;
  course_name?: string;
  deadline?: string;
  status?: string;
  content_type?: string;
}

export interface LunaCachedTarget {
  kind: "announcement" | "report" | "exam" | "discussion" | "thread" | "survey";
  title: string;
  courseName: string;
  path: string;
  idnumber: string;
  infoId: string;
  period: string;
  status: string;
}

interface CourseItemMatch {
  kind: "report" | "exam" | "discussion" | "survey";
  item: LunaContentItem;
}

const jsonCache = new Map<string, unknown>();
let schedulePromise: Promise<ScheduleSnapshot | null> | null = null;
const IGNORED_MATCH_PARAMS = new Set(["_cid", "_csrf", "screen", "directLink", "pageViewListNum", "selectCategoryCd"]);

export function resolveUniversityUrl(value: string): URL | null {
  const raw = String(value || "").trim();
  if (!raw || raw === "#" || raw.startsWith("javascript:")) return null;
  try {
    if (/^https?:\/\//i.test(raw)) return new URL(raw);
    const base = raw.startsWith("/portal/") || raw.startsWith("/cabinet/")
      ? KWIC_BASE
      : raw.startsWith("/uniasv2/") || raw.startsWith("/campusweb/")
        ? KGC_BASE
        : LUNA_BASE;
    return new URL(raw, base);
  } catch {
    return null;
  }
}

async function readJsonCache<T>(key: string): Promise<T | null> {
  if (jsonCache.has(key)) return jsonCache.get(key) as T | null;
  const raw = await invoke<string | null>("get_data_cache", { key }).catch(() => null);
  let parsed: T | null = null;
  try {
    parsed = raw ? JSON.parse(raw) as T : null;
  } catch {}
  jsonCache.set(key, parsed);
  return parsed;
}

async function readSchedule(): Promise<ScheduleSnapshot | null> {
  if (!schedulePromise) {
    schedulePromise = invoke<ScheduleSnapshot>("get_schedule_snapshot").catch(() => null);
  }
  return schedulePromise;
}

function scheduleEntries(snapshot: ScheduleSnapshot | null): ScheduleEntry[] {
  return [
    ...(snapshot?.raw?.kgc_entries_current || []),
    ...(snapshot?.raw?.kgc_entries_next || []),
  ];
}

export async function findKgcPathByCourseName(courseName: string): Promise<string> {
  const name = courseName.trim();
  if (!name) return "";
  const entry = scheduleEntries(await readSchedule())
    .find((item) => item.name?.trim() === name && item.detail_path);
  return entry?.detail_path || "";
}

export async function findKgcEntryByPath(path: string): Promise<ScheduleEntry | null> {
  const normalized = path.trim();
  if (!normalized) return null;
  return scheduleEntries(await readSchedule())
    .find((item) => item.detail_path === normalized) || null;
}

function urlKey(value: string): string {
  const url = resolveUniversityUrl(value);
  if (!url) return "";
  const params = Array.from(url.searchParams.entries())
    .filter(([key]) => !IGNORED_MATCH_PARAMS.has(key))
    .sort(([aKey, aValue], [bKey, bValue]) => aKey === bKey ? aValue.localeCompare(bValue) : aKey.localeCompare(bKey));
  const query = new URLSearchParams(params).toString();
  return `${url.pathname}${query ? `?${query}` : ""}`;
}

function exactParam(value: string | URL, key: string): string {
  const url = value instanceof URL ? value : resolveUniversityUrl(value);
  return url?.searchParams.get(key) || "";
}

function normalizeMatchText(value: string): string {
  return String(value || "")
    .toLowerCase()
    .replace(/[\s\u3000\u00a0]+/g, "")
    .replace(/[|｜:：()（）【】「」『』[\]<>＜＞・,，.．]/g, "");
}

function titlesLooselyMatch(a: string, b: string): boolean {
  const left = normalizeMatchText(a);
  const right = normalizeMatchText(b);
  return !!left && !!right && (left === right || left.includes(right) || right.includes(left));
}

function itemMatches(url: URL, item: LunaContentItem, anchorText = ""): boolean {
  if (urlKey(item.url) === urlKey(url.toString())) return true;
  return ["reportId", "examinationId", "surveyId", "forumId", "threadId"]
    .some((key) => exactParam(url, key) && exactParam(url, key) === exactParam(item.url, key))
    || titlesLooselyMatch(anchorText, item.title);
}

function findCourseItem(course: LunaCourseContents | null, url: URL, anchorText = ""): CourseItemMatch | null {
  if (!course) return null;
  const groups: Array<{ kind: CourseItemMatch["kind"]; items: LunaContentItem[] }> = [
    { kind: "report", items: course.reports || [] },
    { kind: "exam", items: course.examinations || [] },
    { kind: "discussion", items: course.discussions || [] },
    { kind: "survey", items: course.surveys || [] },
  ];
  for (const group of groups) {
    const item = group.items.find((candidate) => itemMatches(url, candidate, anchorText));
    if (item) return { kind: group.kind, item };
  }
  return null;
}

async function readCourse(idnumber: string): Promise<LunaCourseContents | null> {
  if (!idnumber) return null;
  const cached = await readJsonCache<LunaCourseContents>(`luna_course:${idnumber}`);
  if (cached?.course_name) return cached;
  return invoke<LunaCourseContents>("luna_fetch_course_detail", { idnumber }).catch(() => null);
}

async function findTodo(url: URL, anchorText = ""): Promise<CachedTodo | null> {
  const todos = await readJsonCache<CachedTodo[]>("luna_todo");
  if (!Array.isArray(todos)) return null;
  return todos.find((todo) => urlKey(todo.url || "") === urlKey(url.toString()))
    || todos.find((todo) => ["reportId", "examinationId", "surveyId", "forumId", "threadId"]
      .some((key) => exactParam(url, key) && exactParam(url, key) === exactParam(todo.url || "", key)))
    || todos.find((todo) => titlesLooselyMatch(anchorText, todo.content_name || ""))
    || null;
}

export async function resolveLunaCachedTarget(
  url: URL,
  anchorText: string,
  context: LinkContext,
): Promise<LunaCachedTarget | null> {
  const idnumber = url.searchParams.get("idnumber") || context.idnumber || "";
  const course = await readCourse(idnumber);
  const courseName = course?.course_name || context.courseName || "";
  const infoId = url.searchParams.get("informationId") || "";
  const announcement = (course?.announcements || []).find((item) =>
    (infoId && item.info_id === infoId) || titlesLooselyMatch(anchorText, item.title)
  );
  if (announcement) {
    return {
      kind: "announcement",
      title: announcement.title || anchorText,
      courseName,
      path: "",
      idnumber,
      infoId: announcement.info_id || infoId,
      period: "",
      status: "",
    };
  }

  const itemMatch = findCourseItem(course, url, anchorText);
  const item = itemMatch?.item;
  const todo = await findTodo(url, anchorText);
  if (!item && !todo) return null;
  const itemUrl = resolveUniversityUrl(item?.url || todo?.url || url.toString()) || url;
  const inferredKind = itemUrl.searchParams.get("reportId") || url.searchParams.get("reportId")
    ? "report"
    : itemUrl.searchParams.get("examinationId") || url.searchParams.get("examinationId")
      ? "exam"
      : itemUrl.searchParams.get("surveyId") || url.searchParams.get("surveyId")
        ? "survey"
        : itemUrl.searchParams.get("threadId") || url.searchParams.get("threadId")
          ? "thread"
          : itemUrl.searchParams.get("forumId") || url.searchParams.get("forumId")
            ? "discussion"
            : null;
  const kind = itemMatch?.kind === "discussion" && inferredKind === "thread"
    ? "thread"
    : itemMatch?.kind || inferredKind;
  if (!kind) return null;
  return {
    kind,
    title: item?.title || todo?.content_name || anchorText,
    courseName: courseName || todo?.course_name || "",
    path: item?.url || `${url.pathname}${url.search}`,
    idnumber,
    infoId: kind === "report"
      ? itemUrl.searchParams.get("reportId") || url.searchParams.get("reportId") || ""
      : kind === "thread"
        ? itemUrl.searchParams.get("threadId") || url.searchParams.get("threadId") || ""
        : "",
    period: item?.period || todo?.deadline || "",
    status: item?.status || todo?.status || "",
  };
}

export async function resolveCachedRichLinkLabel(
  href: string,
  context: LinkContext,
): Promise<string> {
  const url = resolveUniversityUrl(href);
  if (!url) return "";

  if (url.hostname === "kwic.kwansei.ac.jp") {
    const id = url.searchParams.get("informationId") || "";
    const home = await readJsonCache<{ sections?: Array<{ items?: Array<{ id?: string; title?: string }> }> }>("kwic_home");
    for (const section of home?.sections || []) {
      const item = (section.items || []).find((candidate) => candidate.id === id);
      if (item?.title) return item.title;
    }
  }

  if (url.hostname === "kg-course.kwansei.ac.jp") {
    const entry = await findKgcEntryByPath(`${url.pathname}${url.search}`)
      || await findKgcEntryByPath(url.pathname);
    if (entry?.name) return entry.name;
  }

  if (url.hostname === "luna.kwansei.ac.jp") {
    const idnumber = url.searchParams.get("idnumber") || context.idnumber || "";
    const course = await readCourse(idnumber);
    const courseItem = findCourseItem(course, url);
    if (courseItem?.item.title) return courseItem.item.title;
    const infoId = url.searchParams.get("informationId") || "";
    const announcement = (course?.announcements || []).find((item) => item.info_id === infoId);
    if (announcement?.title) return announcement.title;
    const todo = await findTodo(url);
    if (todo?.content_name) return todo.content_name;
  }

  return "";
}

function isRawLinkLabel(text: string, href: string): boolean {
  const label = text.trim();
  if (!label || label === href.trim()) return true;
  return label === resolveUniversityUrl(href)?.toString();
}

export async function hydrateRichLinkLabels(root: ParentNode, context: LinkContext): Promise<void> {
  const anchors = Array.from(root.querySelectorAll<HTMLAnchorElement>(".rich a[href]"));
  for (const anchor of anchors) {
    const href = anchor.getAttribute("href") || "";
    if (!href || !isRawLinkLabel(anchor.textContent || "", href)) continue;
    const label = await resolveCachedRichLinkLabel(href, context);
    if (!anchor.isConnected || !label) continue;
    anchor.title ||= resolveUniversityUrl(href)?.toString() || href;
    anchor.textContent = label;
  }
}

export async function buildCachedReportFallback(
  path: string,
  currentTitle: string,
  currentCourseName: string,
): Promise<LunaDetailPage | null> {
  const url = resolveUniversityUrl(path);
  if (!url) return null;
  const idnumber = url.searchParams.get("idnumber") || "";
  const course = await readCourse(idnumber);
  const item = findCourseItem(course, url)?.item;
  const todo = await findTodo(url);
  const meta: MetaPair[] = [];
  if (item?.period) meta.push(["公開期間", item.period]);
  if (todo?.deadline) meta.push(["締切", todo.deadline]);
  if (item?.status || todo?.status) meta.push(["状態", item?.status || todo?.status || ""]);
  if (todo?.content_type) meta.push(["種別", todo.content_type]);
  const title = item?.title || todo?.content_name || currentTitle;
  const courseName = course?.course_name || todo?.course_name || currentCourseName;
  if (!title && !courseName && !meta.length) return null;
  meta.push(["詳細", "課題本文は取得できませんでしたが、この課題はローカルキャッシュから特定しました。"]);
  return { title, course_name: courseName, sections: [], attachments: [], meta };
}
