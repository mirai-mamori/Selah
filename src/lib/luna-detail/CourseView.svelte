<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import Icon, { type IconName } from "../Icon.svelte";
  import CourseSupplemental from "./CourseSupplemental.svelte";
  import { lunaDraftKey, readDraft, writeDraft } from "./drafts";
  import type {
    DownloadMark,
    LunaAttendanceItem,
    LunaContentItem,
    LunaCourseAnnouncement,
    LunaCourseContents,
    LunaMaterialFile,
  } from "./types";

  interface Props {
    course: LunaCourseContents;
    mode: string;
    idnumberParam: string;
    kgcPathParam: string;
    split?: boolean;
    richText: (value: string | undefined | null) => string;
    openExternal: (url: string, title?: string) => void | Promise<void>;
    openAnnouncement: (announcement: LunaCourseAnnouncement) => void;
    openMaterialDetail: (item: LunaContentItem) => void;
    openOrDownloadMaterial: (file: LunaMaterialFile, title: string) => void | Promise<void>;
    openDetailWindow: (item: LunaContentItem, mode: string) => void;
    getDownloadMark: (name: string) => DownloadMark;
    reportStatus: (message: string, isError?: boolean) => void;
    onupdate: (course: LunaCourseContents) => void;
  }

  let {
    course,
    mode,
    idnumberParam,
    kgcPathParam,
    split = false,
    richText,
    openExternal,
    openAnnouncement,
    openMaterialDetail,
    openOrDownloadMaterial,
    openDetailWindow,
    getDownloadMark,
    reportStatus,
    onupdate,
  }: Props = $props();

  let attendanceModal = $state<LunaAttendanceItem | null>(null);
  let attendancePass = $state("");
  let attendanceComment = $state("");
  let attendanceBusy = $state(false);
  let attendanceError = $state("");
  let attendanceMeta = $state<{ open_start?: string; open_end?: string; late_start?: string; late_end?: string; content?: string } | null>(null);
  let attendancePassInput = $state<HTMLInputElement | null>(null);

  function shortDate(value: string): string {
    const match = String(value || "").match(/(\d{4})\/(\d{1,2})\/(\d{1,2})/);
    return match ? `${Number(match[2])}/${Number(match[3])}` : value;
  }

  function toolKind(name: string, url = ""): "zoom" | "panopto" | "syllabus" | "default" {
    const text = `${name || ""} ${url || ""}`.toLowerCase();
    if (text.includes("zoom")) return "zoom";
    if (text.includes("panopto")) return "panopto";
    if (text.includes("syllabus") || text.includes("シラバス")) return "syllabus";
    return "default";
  }

  function toolIcon(kind: ReturnType<typeof toolKind>): IconName {
    if (kind === "zoom") return "video";
    if (kind === "panopto") return "broadcast";
    if (kind === "syllabus") return "moon";
    return "globe";
  }

  function parseDeadline(period: string): number {
    const end = String(period || "").split(/[~～]/)[1]?.trim() || "";
    return Date.parse(end.replace(/\//g, "-")) || 0;
  }

  function shortPeriod(period: string): string {
    const end = String(period || "").split(/[~～]/)[1]?.trim() || "";
    const match = end.match(/(\d+)\/(\d+)\/(\d+)\s+(\d+:\d+)/);
    return match ? `${Number(match[2])}/${Number(match[3])} ${match[4]}` : (end || period);
  }

  function taskRemaining(period: string, status: string): string {
    if (status && !status.includes("未")) return status;
    const deadline = parseDeadline(period);
    if (!deadline) return "";
    const diff = deadline - Date.now();
    if (diff <= 0) {
      const elapsed = -diff;
      if (elapsed < 3600000) return `${Math.floor(elapsed / 60000)}分超過`;
      if (elapsed < 86400000) return `${Math.floor(elapsed / 3600000)}時間超過`;
      return `${Math.floor(elapsed / 86400000)}日超過`;
    }
    if (diff < 3600000) return `残り${Math.ceil(diff / 60000)}分`;
    if (diff < 86400000) return `残り${Math.floor(diff / 3600000)}時間`;
    return `残り${Math.floor(diff / 86400000)}日`;
  }

  function taskUrgency(period: string, status: string): "overdue" | "critical" | "soon" | "normal" | "done" {
    if (status && !status.includes("未")) return "done";
    const deadline = parseDeadline(period);
    if (!deadline) return "normal";
    const diff = deadline - Date.now();
    if (diff <= 0) return "overdue";
    if (diff < 86400000) return "critical";
    if (diff < 4 * 86400000) return "soon";
    return "normal";
  }

  function urgencyOrder(period: string, status: string): number {
    return { overdue: 0, critical: 1, soon: 2, normal: 3, done: 4 }[taskUrgency(period, status)];
  }

  // Fill ratio for the urgency bar (mirrors the main TODO view): 0 when far away,
  // grows to 1 as the deadline nears; done tasks read as full.
  function taskUrgencyPct(period: string, status: string): number {
    if (status && !status.includes("未")) return 1;
    const deadline = parseDeadline(period);
    if (!deadline) return 0;
    const diff = deadline - Date.now();
    if (diff <= 0) return 1;
    const horizon = 7 * 86400000;
    if (diff >= horizon) return 0;
    return 1 - diff / horizon;
  }

  function statusClass(status: string): string {
    const s = status || "";
    if (!s) return "";
    // 未出席/欠席 → red, 未登録/登録期間外 → gray, 出席 → green, 受付中 → blue.
    if (s.includes("欠席") || s.includes("未出席")) return "absent";
    if (s.includes("期間外") || s.includes("対象外") || s.includes("未登録") || s.includes("未受付")) return "inactive";
    if (s.includes("遅刻")) return "late";
    if (s.includes("出席") || s.includes("登録済") || s.includes("提出済") || s.includes("回答済")) return "present";
    if (s.includes("受付") || s.includes("登録可能")) return "open";
    if (s.includes("未")) return "inactive";
    return "done";
  }

  function sortedTasks(items: LunaContentItem[]): LunaContentItem[] {
    return [...(items || [])].sort((a, b) => {
      const urgencyDifference = urgencyOrder(a.period, a.status) - urgencyOrder(b.period, b.status);
      if (urgencyDifference) return urgencyDifference;
      const aDeadline = parseDeadline(a.period) || Number.MAX_SAFE_INTEGER;
      const bDeadline = parseDeadline(b.period) || Number.MAX_SAFE_INTEGER;
      return aDeadline - bDeadline;
    });
  }

  function pendingCount(items: LunaContentItem[]): number {
    return (items || []).filter((item) => item.status?.includes("未")).length;
  }

  function extraFilesLabel(count: number): string {
    return `他 ${count} 件`;
  }

  function handleMaterialCardKeydown(event: KeyboardEvent, item: LunaContentItem): void {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    openMaterialDetail(item);
  }

  function visibleAttendances(): LunaAttendanceItem[] {
    const attendances = course.attendances || [];
    if (mode === "attendance" || attendances.length <= 3) return attendances;
    const dated = attendances
      .map((attendance, index) => ({ index, time: Date.parse(String(attendance.date || "").replace(/\//g, "-")) }))
      .filter((item) => !Number.isNaN(item.time))
      .sort((a, b) => a.time - b.time);
    if (dated.length < 3) return attendances.slice(0, 3);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    let right = dated.findIndex((item) => item.time >= today.getTime());
    if (right < 0) right = dated.length;
    const start = Math.max(0, Math.min(dated.length - 3, right - 1));
    const visible = new Set(dated.slice(start, start + 3).map((item) => item.index));
    return attendances.filter((_, index) => visible.has(index));
  }

  function attendanceDraftKey(attendance: LunaAttendanceItem): string {
    return lunaDraftKey(["attendance-comment", attendance.idnumber || idnumberParam, attendance.attendance_id]);
  }

  async function beginAttendance(attendance: LunaAttendanceItem): Promise<void> {
    attendanceModal = attendance;
    attendancePass = "";
    attendanceComment = readDraft(attendanceDraftKey(attendance));
    attendanceError = "";
    attendanceMeta = null;
    await tick();
    attendancePassInput?.focus();
    const targetId = attendance.idnumber || idnumberParam;
    if (targetId && attendance.attendance_id) {
      invoke<{ already_registered?: boolean; open_start?: string; open_end?: string; late_start?: string; late_end?: string; content?: string }>(
        "luna_prefetch_attendance_form",
        { idnumber: targetId, attendanceId: attendance.attendance_id },
      ).then((info) => {
        if (attendanceModal?.attendance_id !== attendance.attendance_id) return;
        if (info?.already_registered) {
          attendanceModal = null;
          reportStatus("登録済みです");
          return;
        }
        attendanceMeta = info || null;
      }).catch(() => {});
    }
  }

  function dismissAttendance(reportCancellation = true): void {
    if (attendanceBusy) return;
    attendanceModal = null;
    attendanceError = "";
    if (reportCancellation) reportStatus("出勤登録をキャンセルしました");
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (attendanceModal && event.key === "Escape") {
      event.preventDefault();
      dismissAttendance();
    }
  }

  function updateAttendanceComment(value: string): void {
    attendanceComment = value.slice(0, 1300);
    if (attendanceModal) writeDraft(attendanceDraftKey(attendanceModal), attendanceComment);
  }

  async function submitAttendance(): Promise<void> {
    if (!attendanceModal || attendanceBusy) return;
    const targetId = attendanceModal.idnumber || idnumberParam;
    if (!targetId || !attendanceModal.attendance_id) return;
    if (!attendancePass.trim()) {
      attendanceError = "ワンタイムパスワードを入力してください";
      await tick();
      attendancePassInput?.focus();
      return;
    }
    attendanceBusy = true;
    attendanceError = "";
    try {
      const message = await invoke<string>("luna_submit_attendance", {
        idnumber: targetId,
        attendanceId: attendanceModal.attendance_id,
        oneTimePass: attendancePass || null,
        comment: attendanceComment || null,
      });
      reportStatus(message || "出席を登録しました");
      writeDraft(attendanceDraftKey(attendanceModal), "");
      attendanceModal = null;
      onupdate(await invoke<LunaCourseContents>("luna_fetch_course_detail", { idnumber: targetId }));
    } catch (error) {
      attendanceError = String(error);
    } finally {
      attendanceBusy = false;
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if mode === "attendance"}
  <h1 class="attendance-title">{course.course_name}</h1>
{:else}
  {@const heroTools = (course.online_tools || []).filter((tool) => toolKind(tool.name, tool.url) === "default")}
  <section class="hero">
    <div class="hero-main">
      <h1>{course.course_name}</h1>
      <div class="hero-meta">
        {#if course.semester}<span class="semester">{course.semester}</span>{/if}
        {#if course.teachers}<span class="teacher"><Icon name="book" size={16} />{course.teachers}</span>{/if}
        {#if course.ta_info}<span class="staff"><strong>TA</strong>{course.ta_info}</span>{/if}
        {#if course.la_info}<span class="staff"><strong>LA</strong>{course.la_info}</span>{/if}
      </div>
    </div>
    {#if heroTools.length}
      <div class="hero-actions">
        {#each heroTools as tool}
          <button class="hero-action default" type="button" onclick={() => openExternal(tool.url, tool.name)}>
            <Icon name={toolIcon(toolKind(tool.name, tool.url))} size={15} />
            <span>{tool.name}</span>
          </button>
        {/each}
      </div>
    {/if}
  </section>
{/if}

{#if mode !== "attendance" && course.announcements?.length}
  <section class="section">
    <div class="section-head"><h2>お知らせ</h2><span>{course.announcements.length}</span></div>
    <div class="grid">
      {#each course.announcements as announcement}
        <button class="item-card" type="button" onclick={() => openAnnouncement(announcement)}>
          <span class="item-row">
            <span class="item-title">{announcement.title}</span>
            {#if announcement.is_new}<span class="item-badge new">NEW</span>{/if}
          </span>
          <span class="item-sub">{shortDate(announcement.start_date)}</span>
        </button>
      {/each}
    </div>
  </section>
{/if}

{#if course.attendances?.length}
  <section class="section">
    <div class="section-head">
      <h2>出席</h2>
      <span>{course.attendances.length}</span>
      {#if mode !== "attendance" && course.attendances.length > 3}
        <button class="small-link" type="button" onclick={() => invoke("university_open_detail_window", {
          path: "", title: "出席", mode: "attendance", period: null, status: null,
          idnumber: idnumberParam, infoId: null, kgcPath: null, courseName: course.course_name, split
        })}>全て</button>
      {/if}
    </div>
    <div class="attendance-list">
      {#each visibleAttendances() as attendance, index}
        <div class="attendance-card">
          <div>
            <strong>{attendance.title || `#${index + 1}`}</strong>
            {#if attendance.date}<span>{attendance.date}</span>{/if}
          </div>
          <div class="attendance-right">
            <span class={`badge ${statusClass(attendance.status)}`}>{attendance.status || "-"}</span>
            {#if attendance.can_register && attendance.attendance_id}
              <button type="button" onclick={() => beginAttendance(attendance)}>署名登録</button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </section>
{:else if mode === "attendance"}
  <section class="section">
    <div class="section-head"><h2>出席</h2><span>0</span></div>
    <div class="empty-state">出席情報がありません</div>
  </section>
{/if}

{#if mode !== "attendance"}
  {#if course.materials?.length}
    <section class="section">
      <div class="section-head"><h2>教材</h2><span>{course.materials.length}</span></div>
      <div class="grid materials">
        {#each course.materials as item}
          <div
            class="material-card"
            role="button"
            tabindex="0"
            aria-label={`${item.title} を開く`}
            onclick={() => openMaterialDetail(item)}
            onkeydown={(event) => handleMaterialCardKeydown(event, item)}
          >
            <div class="card-hit">
              <span class="item-title">{item.title}</span>
              {#if item.period}<span class="item-sub">{shortDate(item.period)}</span>{/if}
            </div>
            {#if item.description}<div class="rich compact">{@html richText(item.description)}</div>{/if}
            {#if item.files?.length}
              {@const file = item.files[0]}
              <div class="file-list">
                <button type="button" onclick={(event) => { event.stopPropagation(); void openOrDownloadMaterial(file, item.title); }}>
                  <Icon name={(file.link_type && file.link_type !== "file") ? "arrow.up.right.square" : "paperclip"} size={14} />
                  <span>{file.display_name || file.file_name}</span>
                  {#if getDownloadMark(file.display_name || file.file_name).loading}<span>...</span>{/if}
                  {#if getDownloadMark(file.display_name || file.file_name).path}<Icon name="checkmark.circle" size={14} />{/if}
                </button>
                {#if item.files.length > 1}
                  <button class="more-files" type="button" onclick={(event) => { event.stopPropagation(); openMaterialDetail(item); }}>
                    {extraFilesLabel(item.files.length - 1)}
                  </button>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <CourseSupplemental courseName={course.course_name} {idnumberParam} {richText} />

  {#if course.reports?.length}
    <section class="section">
      <div class="section-head"><h2>課題</h2><span>{course.reports.length}</span>{#if pendingCount(course.reports)}<span class="item-badge pending">未提出 {pendingCount(course.reports)}</span>{/if}</div>
      <div class="task-list">
        {#each sortedTasks(course.reports) as item}
          {@const urg = taskUrgency(item.period, item.status)}
          {@const remaining = taskRemaining(item.period, item.status)}
          <button class="task-card" type="button" onclick={() => openDetailWindow(item, "report")}>
            <span class={`urgency-bar ${urg}`}><span class="urgency-fill" style="height: {Math.max(taskUrgencyPct(item.period, item.status) * 100, 6)}%"></span></span>
            <span class="task-body">
              <span class="task-name">{item.title}</span>
              <span class="task-sub">
                {#if item.status}<span class="task-type">{item.status}</span>{/if}
                {#if item.status && item.period}<span class="task-sep"></span>{/if}
                {#if item.period}<span class="task-date">{shortPeriod(item.period)}</span>{/if}
              </span>
            </span>
            {#if remaining}<span class={`remaining ${urg}`}>{remaining}</span>{/if}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if course.examinations?.length}
    <section class="section">
      <div class="section-head"><h2>テスト</h2><span>{course.examinations.length}</span>{#if pendingCount(course.examinations)}<span class="item-badge pending">未回答 {pendingCount(course.examinations)}</span>{/if}</div>
      <div class="task-list">
        {#each sortedTasks(course.examinations) as item}
          {@const urg = taskUrgency(item.period, item.status)}
          {@const remaining = taskRemaining(item.period, item.status)}
          <button class="task-card" type="button" onclick={() => openExternal(item.url, item.title)}>
            <span class={`urgency-bar ${urg}`}><span class="urgency-fill" style="height: {Math.max(taskUrgencyPct(item.period, item.status) * 100, 6)}%"></span></span>
            <span class="task-body">
              <span class="task-name">{item.title}</span>
              <span class="task-sub">
                {#if item.status}<span class="task-type">{item.status}</span>{/if}
                {#if item.status && item.period}<span class="task-sep"></span>{/if}
                {#if item.period}<span class="task-date">{shortPeriod(item.period)}</span>{/if}
              </span>
            </span>
            {#if remaining}<span class={`remaining ${urg}`}>{remaining}</span>{/if}
            <span class="task-ext" aria-hidden="true"><Icon name="arrow.up.right.square" size={15} /></span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if course.discussions?.length}
    <section class="section">
      <div class="section-head"><h2>掲示板</h2><span>{course.discussions.length}</span></div>
      <div class="grid">
        {#each course.discussions as item}
          <button class="item-card" type="button" onclick={() => openDetailWindow(item, "discussion")}>
            <span class="item-title">{item.title}</span>
            <span class="item-sub">{shortPeriod(item.period)} {item.status}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}
  {#if course.surveys?.length}
    <section class="section">
      <div class="section-head"><h2>アンケート</h2><span>{pendingCount(course.surveys) ? `${pendingCount(course.surveys)}/${course.surveys.length}` : course.surveys.length}</span></div>
      <div class="grid">
        {#each sortedTasks(course.surveys) as item}
          <button class="item-card" type="button" onclick={() => openDetailWindow(item, "survey")}>
            <span class="item-title">{item.title}</span>
            <span class="item-sub">{shortPeriod(item.period)} {item.status}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}
{/if}

{#if attendanceModal}
  <div
    class="modal-backdrop attendance-modal-backdrop"
    role="presentation"
    onclick={(event) => {
      if (event.target === event.currentTarget) dismissAttendance();
    }}
  >
    <div class="modal attendance-modal" role="dialog" aria-modal="true" aria-labelledby="attendance-modal-title">
      <header class="attendance-modal-header">
        <div class="attendance-modal-titles">
          <h2 id="attendance-modal-title">{attendanceModal.title || "出席登録"}</h2>
          {#if course.course_name || attendanceModal.date}
            <span class="attendance-modal-course">
              {course.course_name || ""}{course.course_name && attendanceModal.date ? " · " : ""}{attendanceModal.date || ""}
            </span>
          {/if}
        </div>
        <button
          class="attendance-modal-close"
          type="button"
          title="閉じる"
          aria-label="閉じる"
          disabled={attendanceBusy}
          onclick={() => dismissAttendance()}
        >
          <Icon name="xmark" size={13} />
        </button>
      </header>

      {#if attendanceMeta && (attendanceMeta.open_start || attendanceMeta.open_end || attendanceMeta.late_start || attendanceMeta.late_end || attendanceMeta.content)}
        <div class="attendance-modal-meta">
          {#if attendanceMeta.open_start || attendanceMeta.open_end}<div><span>受付時間</span><strong>{attendanceMeta.open_start || ""} ～ {attendanceMeta.open_end || ""}</strong></div>{/if}
          {#if attendanceMeta.late_start || attendanceMeta.late_end}<div><span>遅刻時間</span><strong>{attendanceMeta.late_start || ""} ～ {attendanceMeta.late_end || ""}</strong></div>{/if}
          {#if attendanceMeta.content}<div><span>内容</span><strong>{attendanceMeta.content}</strong></div>{/if}
        </div>
      {/if}

      <div class="attendance-modal-body">
        <label class="attendance-field">
          <span class="attendance-field-label">
            <span>パスワード</span>
            <span class="attendance-required">必須</span>
          </span>
          <input
            bind:this={attendancePassInput}
            class:error={!!attendanceError && !attendancePass.trim()}
            type="text"
            placeholder="担当教員に確認してください"
            autocomplete="off"
            spellcheck="false"
            aria-invalid={!!attendanceError && !attendancePass.trim()}
            bind:value={attendancePass}
            oninput={() => attendanceError = ""}
          />
        </label>

        <label class="attendance-field">
          <span class="attendance-field-label">
            <span>コメント</span>
            <span class="attendance-char-count">{attendanceComment.length} / 1300</span>
          </span>
          <textarea
            rows="3"
            maxlength="1300"
            placeholder="任意"
            value={attendanceComment}
            oninput={(event) => updateAttendanceComment((event.currentTarget as HTMLTextAreaElement).value)}
          ></textarea>
        </label>
      </div>

      {#if attendanceError}<div class="modal-error" role="alert">{attendanceError}</div>{/if}
      <div class="modal-actions">
        <button type="button" disabled={attendanceBusy} onclick={() => dismissAttendance()}>キャンセル</button>
        <button class="primary-btn" type="button" disabled={attendanceBusy} onclick={submitAttendance}>
          <span>{attendanceBusy ? "送信中..." : "出席登録"}</span>
        </button>
      </div>
    </div>
  </div>
{/if}
