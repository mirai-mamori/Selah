<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { fileToPayload } from "./filePayload";
  import ReportSubmissionPanel from "./ReportSubmissionPanel.svelte";

  interface Props {
    idnumberParam: string;
    reportIdParam: string;
    pathParam: string;
    periodParam: string;
    openExternal: (url: string, title?: string) => void | Promise<void>;
  }

  let { idnumberParam, reportIdParam, pathParam, periodParam, openExternal }: Props = $props();

  // Turnitin (and other LTI 1.3 tool) 課題 are submitted by launching the
  // external tool, not through Luna's /lms/course/report/* upload API. Their
  // links carry no reportId, so the normal form can never work — detect them
  // and offer a launch button instead.
  const isLtiReport = $derived(/\/lms\/lti1p3\//i.test(pathParam));
  const isTurnitin = $derived(isLtiReport && /[?&]type=turnitin\b/i.test(pathParam));

  let reportType = $state("");
  let loading = $state(true);
  let unavailable = $state("");
  let text = $state("");
  let file = $state<File | null>(null);
  let busy = $state(false);
  let submitted = $state(false);
  let progress = $state(0);
  let status = $state("");

  const canSubmit = $derived(!busy && !submitted && (!!file || !!text.trim()));

  function draftKey(): string {
    const parts = ["report-text", idnumberParam || "", reportIdParam || pathParam || window.location.search];
    const encoded = parts.filter(Boolean).map((part) => encodeURIComponent(String(part)));
    return `selah-luna-draft:v1:${encoded.join("|")}`;
  }

  function readDraft(): string {
    try {
      return localStorage.getItem(draftKey()) || "";
    } catch {
      return "";
    }
  }

  function writeDraft(value: string): void {
    try {
      if (value) localStorage.setItem(draftKey(), value);
      else localStorage.removeItem(draftKey());
    } catch {}
  }

  function parseDateTime(value: string): Date | null {
    const match = String(value || "").trim().match(/^(\d{4})[/-](\d{1,2})[/-](\d{1,2})\s+(\d{1,2}):(\d{2})$/);
    if (!match) return null;
    const year = Number(match[1]);
    const month = Number(match[2]);
    const day = Number(match[3]);
    const hour = Number(match[4]);
    const minute = Number(match[5]);
    if (!year || !month || !day || hour > 24 || minute > 59 || (hour === 24 && minute !== 0)) return null;
    const date = new Date(year, month - 1, day, hour === 24 ? 0 : hour, minute, 0, 0);
    if (hour === 24) date.setDate(date.getDate() + 1);
    return date;
  }

  function submissionPeriodMessage(beforeStartOnly = false): string {
    const parts = String(periodParam || "").split(/[~～]/).map((part) => part.trim()).filter(Boolean);
    if (parts.length < 2) return "";
    const start = parseDateTime(parts[0]);
    const end = parseDateTime(parts[1]);
    if (!start || !end) return "";
    const now = new Date();
    if (now < start) return `提出開始前です。提出期間: ${parts[0]} ～ ${parts[1]}`;
    if (!beforeStartOnly && now > end) return `提出期間が終了しています。提出期間: ${parts[0]} ～ ${parts[1]}`;
    return "";
  }

  function launchExternal(): void {
    if (!pathParam) return;
    void openExternal(pathParam, isTurnitin ? "Turnitin" : "提出ページ");
  }

  async function load(): Promise<void> {
    if (isLtiReport) {
      // Only surface a period message; the launch button is always available so
      // the student can still open Turnitin (which may accept late submissions).
      unavailable = submissionPeriodMessage();
      loading = false;
      return;
    }
    if (!idnumberParam || !reportIdParam) {
      unavailable = "提出パラメータが不足しています。";
      loading = false;
      return;
    }
    const precheck = submissionPeriodMessage(true);
    if (precheck) {
      unavailable = precheck;
      loading = false;
      return;
    }
    try {
      reportType = await invoke<string>("luna_check_report_type", {
        idnumber: idnumberParam,
        reportId: reportIdParam,
        period: periodParam || null,
      });
      if (reportType === "text" || reportType === "both") text = readDraft();
    } catch (error) {
      unavailable = submissionPeriodMessage() || String(error || "この課題は現在提出できません。");
    } finally {
      loading = false;
    }
  }

  function updateText(value: string): void {
    text = value;
    writeDraft(value);
  }

  function selectFile(value: File | null): void {
    if (!value) return;
    if (value.size > 100 * 1024 * 1024) {
      status = "100MBを超えるファイルは提出できません。";
      return;
    }
    if (value.size <= 0) {
      status = "ファイルサイズが0バイトです。";
      return;
    }
    file = value;
    status = "";
  }

  function clearFile(): void {
    file = null;
    progress = 0;
  }

  async function submit(): Promise<void> {
    if (!idnumberParam || !reportIdParam || !canSubmit) return;
    busy = true;
    status = "";
    progress = 8;
    try {
      let result = "";
      if (file) {
        status = "ファイルを読み込み中...";
        const payload = await fileToPayload(file);
        progress = 45;
        status = "アップロード中...";
        result = await invoke<string>("luna_submit_report", {
          idnumber: idnumberParam,
          reportId: reportIdParam,
          period: periodParam || null,
          fileName: payload.fileName,
          fileBase64: payload.fileBase64,
        });
      } else {
        progress = 45;
        status = "提出中...";
        result = await invoke<string>("luna_submit_report_text", {
          idnumber: idnumberParam,
          reportId: reportIdParam,
          period: periodParam || null,
          submissionText: text.trim(),
        });
      }
      progress = 100;
      status = result || "提出しました";
      submitted = true;
      text = "";
      file = null;
      writeDraft("");
    } catch (error) {
      progress = 0;
      status = `提出エラー: ${String(error)}`;
    } finally {
      busy = false;
    }
  }

  onMount(() => void load());
</script>

<ReportSubmissionPanel
  {reportType}
  {loading}
  {unavailable}
  {text}
  {file}
  {busy}
  {submitted}
  {progress}
  {status}
  {canSubmit}
  external={isLtiReport}
  externalLabel={isTurnitin ? "Turnitin で提出する" : "提出ページを開く"}
  externalNote={isTurnitin
    ? "この課題は Turnitin で提出します。ボタンから Turnitin を開いて提出してください。"
    : "この課題は外部ツールで提出します。ボタンから提出ページを開いてください。"}
  ontextchange={updateText}
  onfilechange={selectFile}
  onclearfile={clearFile}
  onsubmit={submit}
  onlaunch={launchExternal}
/>
