import { invoke } from "@tauri-apps/api/core";
import {
  findKgcEntryByPath,
  findKgcPathByCourseName,
  resolveCachedRichLinkLabel,
  resolveLunaCachedTarget,
  resolveUniversityUrl,
} from "./detailEnrichment";

interface LinkContext {
  courseName?: string;
  idnumber?: string;
}

function lunaPath(url: URL): string {
  return `${url.pathname}${url.search}${url.hash}`;
}

async function openLunaDetail(
  url: URL,
  title: string,
  context: LinkContext,
  mode: string | null,
  infoId: string | null = null,
  split = false,
): Promise<void> {
  await invoke("university_open_detail_window", {
    path: mode === "course" || mode === "attendance" || mode === "announcement" ? "" : lunaPath(url),
    title,
    mode,
    period: null,
    status: null,
    idnumber: url.searchParams.get("idnumber") || context.idnumber || null,
    infoId,
    kgcPath: null,
    courseName: context.courseName || null,
    split,
  });
}

export async function openUniversityRichLink(
  href: string,
  title: string,
  context: LinkContext,
  split = false,
): Promise<boolean> {
  const url = resolveUniversityUrl(href);
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  const label = await resolveCachedRichLinkLabel(href, context) || title.trim() || url.toString();

  if (host === "kwic.kwansei.ac.jp") {
    if (url.pathname.startsWith("/cabinet/reference")) {
      await invoke("kwic_open_cabinet_window", { title: label || "学生キャビネット" });
    } else if (url.pathname.startsWith("/portal/home/information/detail") && url.searchParams.get("informationId")) {
      await invoke("kwic_open_detail_window", {
        title: label || "KWIC",
        informationId: url.searchParams.get("informationId") || "",
        informationType: url.searchParams.get("informationType") || "",
        personCategoryCd: url.searchParams.get("personCategoryCd") || "",
        categoryCd: url.searchParams.get("categoryCd") || "",
        split,
      });
    } else {
      await invoke("kwic_open_link", { url: url.toString(), title: label || "KWIC" });
    }
    return true;
  }

  if (host === "luna.kwansei.ac.jp") {
    if (url.pathname === "/lms/course" || url.pathname === "/lms/contents") {
      const kgcPath = await findKgcPathByCourseName(context.courseName || label);
      await invoke("university_open_detail_window", {
        path: "",
        title: label,
        mode: url.hash === "#attendance" ? "attendance" : "course",
        period: null,
        status: null,
        idnumber: url.searchParams.get("idnumber") || context.idnumber || null,
        infoId: null,
        kgcPath: kgcPath || null,
        courseName: context.courseName || null,
        split,
      });
      return true;
    }
    const cachedTarget = await resolveLunaCachedTarget(url, title, context);
    if (cachedTarget?.kind === "announcement") {
      await openLunaDetail(url, cachedTarget.title || "お知らせ", {
        courseName: cachedTarget.courseName,
        idnumber: cachedTarget.idnumber,
      }, "announcement", cachedTarget.infoId, split);
      return true;
    }
    if (cachedTarget?.kind === "report") {
      await invoke("university_open_detail_window", {
        path: cachedTarget.path,
        title: cachedTarget.title || "課題",
        mode: "report",
        period: cachedTarget.period || null,
        status: cachedTarget.status || null,
        idnumber: cachedTarget.idnumber || null,
        infoId: cachedTarget.infoId || null,
        kgcPath: null,
        courseName: cachedTarget.courseName || null,
        split,
      });
      return true;
    }
    if (cachedTarget?.kind === "exam") {
      await invoke("open_external_url", {
        url: resolveUniversityUrl(cachedTarget.path)?.toString() || url.toString(),
        title: cachedTarget.title || "テスト",
      });
      return true;
    }
    if (cachedTarget?.kind === "discussion" || cachedTarget?.kind === "thread" || cachedTarget?.kind === "survey") {
      await openLunaDetail(
        resolveUniversityUrl(cachedTarget.path) || url,
        cachedTarget.title,
        { courseName: cachedTarget.courseName, idnumber: cachedTarget.idnumber },
        cachedTarget.kind,
        cachedTarget.infoId || null,
        split,
      );
      return true;
    }
    if (url.pathname.startsWith("/lms/coursetop/information/listdetail")) {
      await openLunaDetail(url, label || "お知らせ", context, "announcement", url.searchParams.get("informationId"), split);
    } else if (url.pathname.startsWith("/lms/course/report/submission")) {
      await openLunaDetail(url, label || "課題", context, "report", url.searchParams.get("reportId"), split);
    } else if (url.pathname.startsWith("/lms/course/forums/thread")) {
      await openLunaDetail(url, label || "掲示板スレッド", context, "thread", null, split);
    } else if (url.pathname.startsWith("/lms/course/forums/themetop")) {
      await openLunaDetail(url, label || "掲示板", context, "discussion", null, split);
    } else if (url.pathname.startsWith("/lms/course/surveys")) {
      await openLunaDetail(url, label || "アンケート", context, "survey", null, split);
    } else if (url.pathname.startsWith("/lms/course/inquiry/")) {
      await openLunaDetail(url, label || "お問い合わせ", context, "inquiry", null, split);
    } else {
      await openLunaDetail(url, label, context, null, null, split);
    }
    return true;
  }

  if (host === "kg-course.kwansei.ac.jp") {
    const entry = await findKgcEntryByPath(`${url.pathname}${url.search}`)
      || await findKgcEntryByPath(url.pathname);
    if (entry?.detail_path) {
      await invoke("open_detail_window", { path: entry.detail_path, courseName: entry.name || label, split });
    } else {
      await invoke("kwic_open_link", { url: url.toString(), title: label });
    }
    return true;
  }

  if (host.endsWith(".kwansei.ac.jp") || host === "kwansei.ac.jp") {
    await invoke("kwic_open_link", { url: url.toString(), title: label });
  } else {
    await invoke("open_external_url", { url: url.toString(), title: label });
  }
  return true;
}
