<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { emitTo, listen } from "@tauri-apps/api/event";
  import { onDestroy, onMount, tick } from "svelte";
  // Import the surface stylesheets directly (not via surface.css's @import) so
  // Vite hot-reloads edits to each sub-file.
  import "./luna-detail/surface-base.css";
  import "./luna-detail/surface-content.css";
  import "./luna-detail/surface-themes.css";
  import Icon from "./Icon.svelte";
  import MarkdownImageLightbox from "./MarkdownImageLightbox.svelte";
  import CourseView from "./luna-detail/CourseView.svelte";
  import DiscussionView from "./luna-detail/DiscussionView.svelte";
  import InquiryView from "./luna-detail/InquiryView.svelte";
  import LunaContentDetailView from "./luna-detail/LunaContentDetailView.svelte";
  import PortalDetailViews from "./luna-detail/PortalDetailViews.svelte";
  import SurveyView from "./luna-detail/SurveyView.svelte";
  import {
    attachmentName,
    displaySafeMessage,
    invokeLunaDetailWithRetry,
    KGC_BASE,
    KWIC_BASE,
    LUNA_BASE,
    normalizeKgcUrl,
    normalizeKwicUrl,
    normalizeLunaUrl,
    readDetailParam,
    richText,
  } from "./luna-detail/runtime";
  import { openUniversityRichLink } from "./luna-detail/linkRouting";
  import { buildCachedReportFallback, hydrateRichLinkLabels } from "./luna-detail/detailEnrichment";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "./auxiliarySurfaceTheme";
  import type {
    ControlEvent,
    CourseDetail,
    DownloadMark,
    KwicCabinetReference,
    KwicNotificationDetail,
    LunaAttachment,
    LunaContentItem,
    LunaCourseAnnouncement,
    LunaCourseContents,
    LunaDetailPage,
    LunaDiscussionThread,
    LunaInquiryDetail,
    LunaMaterialFile,
    LunaSurveyDetail,
    MetaPair,
  } from "./luna-detail/types";

  const target = readDetailParam("tabLabel");
  const owner = readDetailParam("ownerLabel") || "document-tabs";
  const mode = (readDetailParam("mode") || "detail").toLowerCase();
  const pathParam = readDetailParam("path");
  const titleParam = readDetailParam("title") || "LUNA";
  const periodParam = readDetailParam("period");
  const statusParam = readDetailParam("status");
  const idnumberParam = readDetailParam("idnumber");
  const infoIdParam = readDetailParam("infoId");
  const reportIdParam = readDetailParam("reportId") || infoIdParam;
  const kgcPathParam = readDetailParam("kgcPath");
  const courseNameParam = readDetailParam("courseName");
  const nameParam = readDetailParam("name") || titleParam;

  // When ON, drilling into a sub-detail opens it in a split child pane beside
  // this one instead of as a new tab. Enabled by default; the right pane drills
  // in place (replacing itself) and never shows the toggle.
  // Split child panes have targets like "…-ct-s1" / "…-ct-s2".
  const isChildPane = /-s\d+$/.test(target);
  let splitMode = $state(true);

  function closeThisPane(): void {
    void invoke("document_tabs_close_pane", { owner, target }).catch(() => {});
  }

  let loading = $state(true);
  let error = $state("");
  let statusText = $state("");
  let course = $state<LunaCourseContents | null>(null);
  let detail = $state<LunaDetailPage | null>(null);
  let survey = $state<LunaSurveyDetail | null>(null);
  let discussion = $state<LunaDiscussionThread | null>(null);
  let inquiry = $state<LunaInquiryDetail | null>(null);
  let kgcDetail = $state<CourseDetail | null>(null);
  let kwicDetail = $state<KwicNotificationDetail | null>(null);
  let kwicCabinet = $state<KwicCabinetReference | null>(null);
  let materialFiles = $state<LunaMaterialFile[]>([]);
  let downloaded = $state<Record<string, DownloadMark>>({});
  let lightboxImage = $state<{ src: string; alt: string } | null>(null);
  let unlistenControl: (() => void) | null = null;
  let themeUnlisten: (() => void) | null = null;
  let appThemeUnlisten: (() => void) | null = null;
  let loginUnlisten: (() => void) | null = null;
  let lastToolbarTitleHint = "";
  let loadRunning = false;

  const pageTitle = $derived(
    course?.course_name
      || detail?.title
      || survey?.title
      || discussion?.title
      || inquiry?.title
      || kwicDetail?.title
      || kwicCabinet?.title
      || (kgcDetail ? nameParam : "")
      || titleParam
  );

  const courseName = $derived(
    course?.course_name
      || detail?.course_name
      || discussion?.course_name
      || inquiry?.course_name
      || courseNameParam
      || ""
  );

  function currentLunaUrl(): string {
    if (mode === "course" && idnumberParam) return `${LUNA_BASE}/lms/course?idnumber=${encodeURIComponent(idnumberParam)}`;
    if (mode === "attendance" && idnumberParam) return `${LUNA_BASE}/lms/course?idnumber=${encodeURIComponent(idnumberParam)}#attendance`;
    if (mode === "announcement" && idnumberParam && infoIdParam) {
      return `${LUNA_BASE}/lms/coursetop/information/listdetail?idnumber=${encodeURIComponent(idnumberParam)}&informationId=${encodeURIComponent(infoIdParam)}`;
    }
    if (pathParam) return normalizeLunaUrl(pathParam);
    return "";
  }

  function currentSourceUrl(): string {
    if (mode === "kwic") return `${KWIC_BASE}/portal/home`;
    if (mode === "kwiccabinet") return `${KWIC_BASE}/cabinet/reference`;
    if (mode === "kgc" && pathParam) return normalizeKgcUrl(pathParam);
    if (mode === "syllabus") return `${KGC_BASE}/uniasv2/AGA030.do`;
    return currentLunaUrl();
  }

  function markKey(name: string): string {
    return name.toLowerCase();
  }

  function setDownloadMark(name: string, mark: DownloadMark): void {
    downloaded = { ...downloaded, [markKey(name)]: mark };
  }

  function getDownloadMark(name: string): DownloadMark {
    return downloaded[markKey(name)] || {};
  }

  function reportStatus(message: string, isError = false): void {
    statusText = message;
    if (isError) error = message;
    window.setTimeout(() => {
      if (statusText === message) statusText = "";
    }, 2200);
  }

  function updateControls(): void {
    const controls = [];
    // Course online tools (Zoom / Panopto) live in the toolbar with their brand
    // color but no filled background.
    (course?.online_tools || []).forEach((tool, index) => {
      const text = `${tool.name || ""} ${tool.url || ""}`.toLowerCase();
      const tone = text.includes("zoom") ? "zoom" : text.includes("panopto") ? "panopto" : "";
      if (!tone || !tool.url) return;
      controls.push({
        id: `tool-${index}-${tone}`,
        label: tool.name || (tone === "zoom" ? "Zoom" : "Panopto"),
        action: "detail.openTool",
        icon: tone === "zoom" ? "video" : "broadcast",
        tone,
        payload: { url: tool.url, name: tool.name },
      });
    });
    if (course?.syllabus_url) {
      controls.push({
        id: "syllabus",
        label: "シラバス",
        action: "detail.openSyllabus",
        icon: "moon",
        tone: "syllabus",
      });
    }
    if (courseName) {
      controls.push({
        id: "materials",
        label: "資料管理",
        action: "detail.openMaterials",
        icon: "folder.open",
      });
    }
    const sourceUrl = currentSourceUrl();
    if (sourceUrl) {
      // Unified across LUNA/KWIC/KGC: opens the original page in the in-app browser.
      controls.push({
        id: "open-source",
        label: "詳細を見る",
        action: "detail.openSource",
        icon: "globe",
        primary: !courseName,
      });
    }
    if (!isChildPane) {
      controls.push({
        id: "split",
        label: splitMode ? "分割を終了" : "分割表示",
        action: "detail.toggleSplit",
        icon: "square.grid.2x2",
        active: splitMode,
        group: "view",
      });
    }
    invoke("document_tabs_set_controls", { owner, target, controls }).catch(() => {});
  }

  function emitToolbarTitleHint(title: string): void {
    if (!target || title === lastToolbarTitleHint) return;
    lastToolbarTitleHint = title;
    emitTo("document-tabs-strip", "document-tab-title-hint", {
      owner,
      target,
      title,
    }).catch(() => {});
  }

  function updateToolbarTitleHint(): void {
    const titleElement = document.querySelector<HTMLElement>(".hero h1, .detail-wrap > h1");
    const scrollRoot = document.querySelector<HTMLElement>(".luna-detail");
    if (!titleElement || !scrollRoot || loading || error) {
      emitToolbarTitleHint("");
      return;
    }
    const titleRect = titleElement.getBoundingClientRect();
    const rootRect = scrollRoot.getBoundingClientRect();
    const hiddenAboveToolbar = titleRect.bottom <= rootRect.top + 8;
    emitToolbarTitleHint(hiddenAboveToolbar ? String(pageTitle || titleParam || "").trim() : "");
  }

  async function openExternal(url: string, title?: string): Promise<void> {
    const normalized = normalizeLunaUrl(url);
    if (!normalized) return;
    await invoke("open_external_url", { url: normalized, title: title || null }).catch((e) => reportStatus(String(e), true));
  }

  async function openKgcExternal(url: string, title?: string): Promise<void> {
    const normalized = normalizeKgcUrl(url);
    if (!normalized) return;
    await invoke("open_external_url", { url: normalized, title: title || null }).catch((e) => reportStatus(String(e), true));
  }

  function extractSyllabusClassCode(url: string): string {
    const raw = String(url || "").trim();
    if (!raw) return "";
    try {
      const parsed = new URL(raw, KGC_BASE);
      const code = parsed.searchParams.get("LSN_CD")
        || parsed.searchParams.get("lsn_cd")
        || parsed.searchParams.get("classCode")
        || parsed.searchParams.get("class_code")
        || parsed.searchParams.get("lsnCd")
        || "";
      if (code) return code.trim();
    } catch {
      // Fall through to regex extraction for malformed or partially encoded links.
    }
    const match = raw.match(/(?:[?&]|^)(?:LSN_CD|lsn_cd|classCode|class_code|lsnCd)=([^&#]+)/i);
    return match ? decodeURIComponent(match[1]).trim() : "";
  }

  async function openKwicLink(url: string, title?: string): Promise<void> {
    const normalized = normalizeKwicUrl(url);
    if (!normalized) return;
    await invoke("kwic_open_link", { url: normalized, title: title || "詳細" }).catch((e) => reportStatus(String(e), true));
  }

  async function openSource(): Promise<void> {
    const url = currentSourceUrl();
    if (!url) return;
    // Open the original page in the in-app browser (KWIC keeps its auth cookies via
    // its dedicated path); the button label stays platform-neutral.
    const fallbackTitle = pageTitle || titleParam || "詳細";
    if (mode === "kwic" || mode === "kwiccabinet") await openKwicLink(url, fallbackTitle);
    else if (mode === "kgc" || mode === "syllabus") await openKgcExternal(url, fallbackTitle);
    else await openExternal(url, fallbackTitle);
  }

  async function openSyllabus(): Promise<void> {
    const url = course?.syllabus_url;
    if (!url) return;
    const classCode = extractSyllabusClassCode(url);
    if (classCode) {
      await invoke("open_syllabus_detail", {
        classCode,
        courseName: courseName || pageTitle || "シラバス",
      }).catch((e) => reportStatus(String(e), true));
      return;
    }
    await openKgcExternal(url, "シラバス");
  }

  function isCurrentControlEvent(control: ControlEvent | undefined): boolean {
    return control?.owner === owner && control?.target === target;
  }

  function handleControl(event: { payload: ControlEvent }): void {
    const control = event.payload;
    if (!isCurrentControlEvent(control)) return;
    const action = control.action;
    if (action === "detail.openSource") void openSource();
    else if (action === "detail.openMaterials") void invoke("open_files_tab", { focusCourse: courseName || null });
    else if (action === "detail.openSyllabus") void openSyllabus();
    else if (action === "detail.openTool") {
      const payload = control.payload as { url?: string; name?: string } | undefined;
      if (payload?.url) void openExternal(payload.url, payload.name);
    }
    else if (action === "detail.toggleSplit") toggleSplit();
  }

  function toggleSplit(): void {
    splitMode = !splitMode;
    if (!splitMode) {
      void invoke("document_tabs_close_split", { owner }).catch(() => {});
    }
    updateControls();
  }

  function handleRichLinkClick(event: MouseEvent): void {
    if (!(event.target instanceof Element)) return;
    const image = event.target.closest<HTMLImageElement>(".rich img");
    if (image) {
      event.preventDefault();
      event.stopPropagation();
      lightboxImage = { src: image.currentSrc || image.src, alt: image.alt || "" };
      return;
    }
    const anchor = event.target.closest<HTMLAnchorElement>(".rich a[href]");
    if (!anchor) return;
    const href = anchor.getAttribute("href") || "";
    if (!href || href === "#") return;
    event.preventDefault();
    event.stopPropagation();
    void openUniversityRichLink(href, (anchor.textContent || "").trim(), {
      courseName,
      idnumber: idnumberParam,
    }, splitMode).catch((error) => reportStatus(String(error), true));
  }

  function handleTabActivated(): void {
    updateToolbarTitleHint();
    if (error && mode !== "kwic" && mode !== "kwiccabinet" && mode !== "kgc" && mode !== "syllabus") {
      void load();
    }
  }

  function reportFallbackData(message = "課題本文は取得できませんでしたが、提出フォームは利用できます。"): LunaDetailPage {
    const meta: MetaPair[] = [];
    if (periodParam) meta.push(["公開期間", periodParam]);
    if (statusParam) meta.push(["状態", statusParam]);
    meta.push(["詳細", message]);
    return {
      title: titleParam,
      course_name: courseNameParam,
      sections: [],
      attachments: [],
      meta,
    };
  }

  async function checkDownloaded(names: string[]): Promise<void> {
    const unique = [...new Set(names.map((name) => name.trim()).filter(Boolean))];
    if (!unique.length) return;
    try {
      const found = await invoke<Record<string, { file_exists?: boolean; path?: string }>>("check_files_downloaded", {
        filenames: unique,
        courseName: courseName || null,
      });
      const next = { ...downloaded };
      for (const name of unique) {
        const rec = found?.[name] || found?.[name.toLowerCase()];
        if (rec?.file_exists && rec.path) next[markKey(name)] = { path: rec.path };
      }
      downloaded = next;
    } catch {}
  }

  async function downloadAttachment(att: LunaAttachment): Promise<void> {
    const name = attachmentName(att);
    const current = getDownloadMark(name);
    if (current.path) {
      await invoke("open_downloaded_file", { path: current.path }).catch((e) => reportStatus(`ファイルを開けません: ${String(e)}`, true));
      return;
    }
    await forceDownloadAttachment(att);
  }

  async function forceDownloadAttachment(att: LunaAttachment): Promise<void> {
    const name = attachmentName(att);
    if (getDownloadMark(name).loading) return;
    setDownloadMark(name, { loading: true });
    try {
      const result = await invoke<string>("luna_download_file", {
        url: att.url || "",
        filename: name,
        pagePath: pathParam || null,
        objectName: att.object_name || null,
        downloadAction: att.download_action || null,
        downloadParams: att.download_params || null,
        courseName: courseName || null,
        detailTitle: pageTitle || null,
      });
      if (/^https?:\/\//i.test(result)) {
        setDownloadMark(name, {});
        await openExternal(result, name);
      } else {
        setDownloadMark(name, { path: result });
        await invoke("luna_reveal_file", { path: result }).catch(() => {});
      }
    } catch (e) {
      setDownloadMark(name, {});
      reportStatus(`ダウンロードエラー: ${String(e)}`, true);
    }
  }

  async function openOrDownloadMaterial(file: LunaMaterialFile, materialTitle: string): Promise<void> {
    // An external_url means this is a link to follow (e.g. a material/resource
    // page), not a managed file download — open it in the browser even when the
    // file_type would otherwise mark it as a downloadable file.
    const linkType = file.external_url
      ? "web"
      : file.link_type || (file.file_type === "0" ? "file" : "web");
    if (linkType !== "file") {
      try {
        // A relative Luna link (e.g. /lms/course/display/material/resource?…)
        // just needs its host prefixed; only fall back to rebuilding the URL
        // from object fields when there's no usable link at all.
        const relativeLink = file.external_url?.startsWith("/") ? normalizeLunaUrl(file.external_url) : "";
        const url = file.external_url && /^https?:/i.test(file.external_url)
          ? file.external_url
          : relativeLink
          ? relativeLink
          : await invoke<string>("luna_resolve_material_link", {
              idnumber: idnumberParam,
              fileName: file.file_name,
              objectName: file.object_name,
              resourceId: file.resource_id,
              fileType: file.file_type || "0",
              materialId: file.material_id || null,
              displayName: file.display_name || null,
              endDate: file.end_date || null,
            });
        await openExternal(url, file.display_name || materialTitle);
      } catch (e) {
        reportStatus(`リンクを開けませんでした: ${String(e)}`, true);
      }
      return;
    }

    const name = file.display_name || file.file_name;
    const current = getDownloadMark(name);
    if (current.path) {
      await invoke("open_downloaded_file", { path: current.path }).catch((e) => reportStatus(String(e), true));
      return;
    }
    setDownloadMark(name, { loading: true });
    try {
      const path = await invoke<string>("luna_download_material", {
        idnumber: idnumberParam,
        fileName: file.file_name || file.display_name,
        objectName: file.object_name,
        resourceId: file.resource_id,
        fileType: file.file_type || "0",
        materialId: file.material_id || null,
        displayName: file.display_name || null,
        endDate: file.end_date || null,
        courseName: courseName || null,
        materialTitle: materialTitle || null,
      });
      setDownloadMark(name, { path });
      await invoke("luna_reveal_file", { path }).catch(() => {});
    } catch (e) {
      setDownloadMark(name, {});
      reportStatus(`ダウンロードエラー: ${String(e)}`, true);
    }
  }

  function openDetailWindow(item: LunaContentItem, nextMode: string): void {
    if (!item.url && nextMode !== "material") return;
    const query = new URLSearchParams(item.url.split("?")[1] || "");
    invoke("university_open_detail_window", {
      path: item.url || "",
      title: item.title,
      mode: nextMode,
      period: item.period || null,
      status: item.description || item.status || null,
      idnumber: idnumberParam || query.get("idnumber") || null,
      infoId: nextMode === "report" ? query.get("reportId") : null,
      kgcPath: null,
      courseName: courseName || null,
      split: splitMode,
    }).catch((e) => reportStatus(String(e), true));
  }

  function openMaterialDetail(item: LunaContentItem): void {
    invoke("university_open_detail_window", {
      path: "",
      title: item.title,
      mode: "material",
      period: item.period || null,
      status: item.description || null,
      idnumber: idnumberParam || null,
      infoId: item.files?.length ? JSON.stringify(item.files) : null,
      kgcPath: null,
      courseName: courseName || null,
      split: splitMode,
    }).catch((e) => reportStatus(String(e), true));
  }

  function openAnnouncement(ann: LunaCourseAnnouncement): void {
    if (!ann.info_id || !idnumberParam) return;
    invoke("university_open_detail_window", {
      path: "",
      title: ann.title,
      mode: "announcement",
      period: null,
      status: null,
      idnumber: idnumberParam,
      infoId: ann.info_id,
      kgcPath: null,
      courseName: courseName || null,
      split: splitMode,
    }).catch((e) => reportStatus(String(e), true));
  }

  async function waitForSyllabusDetail(): Promise<CourseDetail> {
    try {
      return await invoke<CourseDetail>("get_syllabus_detail", { label: target });
    } catch {
      // The backend opens the tab before the slow KGC fetch finishes, then emits
      // a label-scoped event to this content webview.
    }
    return new Promise<CourseDetail>(async (resolve, reject) => {
      let settled = false;
      let readyUnlisten: (() => void) | null = null;
      let errorUnlisten: (() => void) | null = null;
      const timer = window.setTimeout(() => {
        if (settled) return;
        settled = true;
        readyUnlisten?.();
        errorUnlisten?.();
        reject(new Error("シラバス詳細の取得がタイムアウトしました"));
      }, 45000);

      const finish = (fn: () => void): void => {
        if (settled) return;
        settled = true;
        window.clearTimeout(timer);
        readyUnlisten?.();
        errorUnlisten?.();
        fn();
      };

      readyUnlisten = await listen<string>("syllabus-ready", async (event) => {
        if (event.payload && event.payload !== target) return;
        try {
          const detail = await invoke<CourseDetail>("get_syllabus_detail", { label: target });
          finish(() => resolve(detail));
        } catch (e) {
          finish(() => reject(e));
        }
      });
      errorUnlisten = await listen<string>("syllabus-error", (event) => {
        finish(() => reject(new Error(event.payload || "シラバス詳細を取得できませんでした")));
      });
    });
  }

  async function load(): Promise<void> {
    if (loadRunning) return;
    loadRunning = true;
    loading = true;
    error = "";
    try {
      if (mode === "course" || mode === "attendance") {
        if (!idnumberParam) throw new Error("idnumber がありません");
        course = await invoke<LunaCourseContents>("luna_fetch_course_detail", { idnumber: idnumberParam });
        document.title = course.course_name || titleParam;
        await checkDownloaded((course.materials || []).flatMap((item) => (item.files || []).map((file) => file.display_name || file.file_name)));
      } else if (mode === "material") {
        detail = {
          title: titleParam,
          course_name: courseNameParam,
          sections: statusParam ? [{ heading: "", body: statusParam }] : [],
          attachments: [],
          meta: periodParam ? [["公開期間", periodParam]] : [],
        };
        try {
          materialFiles = infoIdParam ? JSON.parse(infoIdParam) as LunaMaterialFile[] : [];
        } catch {
          materialFiles = [];
        }
        await checkDownloaded(materialFiles.map((file) => file.display_name || file.file_name));
      } else if (mode === "announcement") {
        if (!idnumberParam || !infoIdParam) throw new Error("お知らせパラメータが不足しています");
        detail = await invokeLunaDetailWithRetry<LunaDetailPage>("luna_fetch_announcement_detail", {
          idnumber: idnumberParam,
          infoId: infoIdParam,
          expectedTitle: titleParam,
        });
        await checkDownloaded((detail.attachments || []).map(attachmentName));
      } else if (mode === "report") {
        if (!pathParam) throw new Error("パスが指定されていません");
        try {
          detail = await invokeLunaDetailWithRetry<LunaDetailPage>("luna_fetch_detail", { path: pathParam, expectedTitle: titleParam });
        } catch {
          detail = await buildCachedReportFallback(pathParam, titleParam, courseNameParam) || reportFallbackData();
        }
        if (!detail.title) detail.title = titleParam;
        if (!detail.course_name && courseNameParam) detail.course_name = courseNameParam;
        if (!(detail.sections?.length || detail.meta?.length || detail.attachments?.length)) {
          detail = await buildCachedReportFallback(pathParam, detail.title || titleParam, detail.course_name || courseNameParam)
            || reportFallbackData();
        }
        await checkDownloaded((detail.attachments || []).map(attachmentName));
      } else if (mode === "discussion") {
        discussion = await invoke<LunaDiscussionThread>("luna_fetch_discussion_detail", { url: pathParam });
      } else if (mode === "thread") {
        discussion = await invoke<LunaDiscussionThread>("luna_fetch_thread_posts", { url: pathParam });
        await checkDownloaded((discussion.posts || []).flatMap((post) => (post.attachments || []).map(attachmentName)));
      } else if (mode === "survey" || mode === "questionnaire") {
        survey = await invoke<LunaSurveyDetail>("luna_fetch_survey_detail", { path: pathParam });
        await checkDownloaded((survey.attachments || []).map((att) => att.file_name));
      } else if (mode === "inquiry") {
        inquiry = await invoke<LunaInquiryDetail>("luna_fetch_inquiry_detail", { path: pathParam });
        await checkDownloaded((inquiry.posts || []).flatMap((post) => (post.attachments || []).map(attachmentName)));
      } else if (mode === "kwic") {
        kwicDetail = await invoke<KwicNotificationDetail>("kwic_fetch_detail", {
          informationId: infoIdParam || readDetailParam("informationId"),
          informationType: readDetailParam("informationType") || "",
          personCategoryCd: readDetailParam("personCategoryCd") || "",
          categoryCd: readDetailParam("categoryCd") || "",
        });
      } else if (mode === "kwiccabinet") {
        kwicCabinet = await invoke<KwicCabinetReference>("kwic_fetch_cabinet_reference");
      } else if (mode === "kgc") {
        if (!pathParam) throw new Error("KGC詳細の path がありません");
        kgcDetail = await invoke<CourseDetail>("fetch_course_detail", { path: pathParam });
      } else if (mode === "syllabus") {
        kgcDetail = await waitForSyllabusDetail();
      } else {
        if (!pathParam) throw new Error("詳細ページの path がありません");
        detail = await invokeLunaDetailWithRetry<LunaDetailPage>("luna_fetch_detail", { path: pathParam, expectedTitle: titleParam });
        await checkDownloaded((detail.attachments || []).map(attachmentName));
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      loadRunning = false;
      updateControls();
      await tick();
      void hydrateRichLinkLabels(document, { courseName, idnumber: idnumberParam });
      updateToolbarTitleHint();
    }
  }

  onMount(async () => {
    await syncAuxiliaryTheme();
    themeUnlisten = await listen<string>("theme-changed", (event) => applyAuxiliaryTheme(event.payload));
    appThemeUnlisten = await listen("app-theme-changed", () => void syncAuxiliaryTheme());
    loginUnlisten = await listen("luna-login-success", () => {
      if (mode !== "kwic" && mode !== "kwiccabinet" && mode !== "kgc" && mode !== "syllabus") void load();
    }).catch(() => null);
    unlistenControl = await listen<ControlEvent>("document-tab-control", handleControl);
    window.addEventListener("selah-tab-activated", handleTabActivated);
    updateControls();
    await load();
  });

  onDestroy(() => {
    themeUnlisten?.();
    appThemeUnlisten?.();
    loginUnlisten?.();
    unlistenControl?.();
    window.removeEventListener("selah-tab-activated", handleTabActivated);
    emitToolbarTitleHint("");
    invoke("document_tabs_set_controls", { owner, target, controls: [] }).catch(() => {});
  });
</script>

<svelte:head>
  <title>{pageTitle || "LUNA"}</title>
</svelte:head>

<main class="luna-detail" onclickcapture={handleRichLinkClick} onscroll={updateToolbarTitleHint}>
  {#if isChildPane}
    <button class="split-pane-close" type="button" title="このペインを閉じる" aria-label="このペインを閉じる" onclick={closeThisPane}>
      <Icon name="xmark" size={14} />
    </button>
  {/if}
  {#if loading}
    <div class="state"><span class="spinner"></span>読み込み中...</div>
  {:else if error}
    <div class="state error-state">
      <Icon name="exclamationmark.triangle" size={24} />
      <strong>読み込みに失敗しました</strong>
      <span>{displaySafeMessage(error)}</span>
    </div>
  {:else}
    {#if course}
      <CourseView
        {course}
        {mode}
        {idnumberParam}
        {kgcPathParam}
        split={splitMode}
        {richText}
        {openExternal}
        {openAnnouncement}
        {openMaterialDetail}
        {openOrDownloadMaterial}
        {openDetailWindow}
        {getDownloadMark}
        {reportStatus}
        onupdate={(value) => course = value}
      />
    {/if}

    {#if detail}
      <LunaContentDetailView
        {detail}
        {materialFiles}
        {mode}
        {titleParam}
        {idnumberParam}
        {reportIdParam}
        {pathParam}
        {periodParam}
        {richText}
        {openOrDownloadMaterial}
        {getDownloadMark}
        {openExternal}
        {attachmentName}
        {downloadAttachment}
        {forceDownloadAttachment}
      />
    {/if}

    {#if survey}
      <SurveyView
        {survey}
        {titleParam}
        {pathParam}
        {richText}
        {downloadAttachment}
        {forceDownloadAttachment}
        {getDownloadMark}
      />
    {/if}

    {#if discussion}
      <DiscussionView
        {discussion}
        {mode}
        {pathParam}
        {titleParam}
        {idnumberParam}
        {courseName}
        split={splitMode}
        {richText}
        {attachmentName}
        {downloadAttachment}
        {forceDownloadAttachment}
        {getDownloadMark}
        {reportStatus}
        onupdate={(value) => discussion = value}
      />
    {/if}

    {#if inquiry}
      <InquiryView
        {inquiry}
        {pathParam}
        {titleParam}
        {richText}
        {attachmentName}
        {downloadAttachment}
        {forceDownloadAttachment}
        {getDownloadMark}
        {reportStatus}
        onupdate={(value) => inquiry = value}
      />
    {/if}

    <PortalDetailViews
      {kwicDetail}
      {kwicCabinet}
      {kgcDetail}
      {mode}
      {titleParam}
      {nameParam}
      {richText}
      {openKwicLink}
    />

    {#if statusText && !error}
      <div class="toast">{statusText}</div>
    {/if}

    {#if lightboxImage}
      <MarkdownImageLightbox src={lightboxImage.src} alt={lightboxImage.alt} onclose={() => lightboxImage = null} />
    {/if}
  {/if}

</main>
