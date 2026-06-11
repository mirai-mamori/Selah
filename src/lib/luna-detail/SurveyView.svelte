<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import AttachmentDownloadRow from "./AttachmentDownloadRow.svelte";
  import { lunaDraftKey, readDraft, writeDraft } from "./drafts";
  import type { DownloadMark, LunaAttachment, LunaSurveyDetail } from "./types";

  interface Props {
    survey: LunaSurveyDetail;
    titleParam: string;
    pathParam: string;
    richText: (value: string | undefined | null) => string;
    downloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    forceDownloadAttachment: (attachment: LunaAttachment) => void | Promise<void>;
    getDownloadMark: (name: string) => DownloadMark;
  }

  let { survey, titleParam, pathParam, richText, downloadAttachment, forceDownloadAttachment, getDownloadMark }: Props = $props();
  let answers = $state<Record<string, string | string[]>>({});
  let busy = $state(false);
  let status = $state("");
  let submitted = $state(false);
  let failed = $state(false);
  const formAvailable = $derived(!!survey.form_fields?.length);

  function attachmentValue(attachment: LunaSurveyDetail["attachments"][number]): LunaAttachment {
    return {
      name: attachment.file_name,
      file_name: attachment.file_name,
      url: attachment.url,
      object_name: attachment.object_name,
      download_action: attachment.download_action,
      download_params: attachment.download_params,
    };
  }

  function valueFor(index: number): string {
    const value = answers[String(index)];
    return typeof value === "string" ? value : "";
  }

  function setValue(index: number, value: string): void {
    answers = { ...answers, [String(index)]: value };
    const question = survey.questions[index];
    if (question?.answer_type === "textarea" || !["list", "checkbox", "radio"].includes(question?.answer_type || "")) {
      writeDraft(lunaDraftKey(["survey-text", pathParam || window.location.search, index]), value);
    }
  }

  function toggleCheckbox(index: number, value: string, checked: boolean): void {
    const key = String(index);
    const current = Array.isArray(answers[key]) ? answers[key] as string[] : [];
    const next = checked ? [...new Set([...current, value])] : current.filter((item) => item !== value);
    answers = { ...answers, [key]: next };
  }

  function isChecked(index: number, value: string): boolean {
    const current = answers[String(index)];
    return Array.isArray(current) && current.includes(value);
  }

  async function submit(): Promise<void> {
    if (busy || submitted || !formAvailable) return;
    const missing = survey.questions
      .map((question, index) => ({ question, index }))
      .filter(({ question, index }) => {
        if (!question.required) return false;
        const value = answers[String(index)];
        return Array.isArray(value) ? value.length === 0 : !String(value || "").trim();
      })
      .map(({ question }) => `Q${question.number}`);
    if (missing.length) {
      status = `未回答: ${missing.join(", ")}`;
      return;
    }

    busy = true;
    status = "";
    failed = false;
    const payload: Record<string, { name: string; value: string | string[] }> = {};
    survey.questions.forEach((question, index) => {
      payload[String(index)] = {
        name: question.answer_name || "",
        value: answers[String(index)] || (question.answer_type === "checkbox" ? [] : ""),
      };
    });

    try {
      await invoke("luna_submit_survey", {
        formFields: survey.form_fields || [],
        answers: payload,
        submitPath: survey.form_action || "/lms/course/surveys/take",
        refererPath: pathParam || survey.form_action || "/lms/course/surveys/take",
      });
      survey.questions.forEach((question, index) => {
        if (question.answer_type === "textarea" || !["list", "checkbox", "radio"].includes(question.answer_type || "")) {
          writeDraft(lunaDraftKey(["survey-text", pathParam || window.location.search, index]), "");
        }
      });
      status = "回答が提出されました";
      submitted = true;
    } catch (error) {
      status = `提出エラー: ${String(error)}`;
      failed = true;
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    const restored: Record<string, string | string[]> = {};
    survey.questions.forEach((question, index) => {
      if (question.answer_type === "textarea" || !["list", "checkbox", "radio"].includes(question.answer_type || "")) {
        const value = readDraft(lunaDraftKey(["survey-text", pathParam || window.location.search, index]));
        if (value) restored[String(index)] = value;
      }
    });
    if (Object.keys(restored).length) answers = { ...answers, ...restored };
  });
</script>

<article class="detail-wrap">
  <h1>{survey.title || titleParam}</h1>
  {#if survey.description}<div class="rich lead">{@html richText(survey.description)}</div>{/if}
  <div class="chips">
    {#if survey.period}<span>期間 <strong>{survey.period}</strong></span>{/if}
    {#if survey.anonymity}<span>匿名性 <strong>{survey.anonymity}</strong></span>{/if}
    {#if survey.allow_edit}<span>回答の変更 <strong>{survey.allow_edit}</strong></span>{/if}
    {#if survey.answer_status}<span>状態 <strong>{survey.answer_status}</strong></span>{/if}
    {#if survey.respondent}<span>回答者 <strong>{survey.respondent}</strong></span>{/if}
  </div>

  {#if survey.attachments?.length}
    <div class="attachments">
      <h2>添付ファイル</h2>
      {#each survey.attachments as attachment}
        <AttachmentDownloadRow
          name={attachment.file_name}
          mark={getDownloadMark(attachment.file_name)}
          onopen={() => downloadAttachment(attachmentValue(attachment))}
          onredownload={() => forceDownloadAttachment(attachmentValue(attachment))}
        />
      {/each}
    </div>
  {/if}

  {#if survey.questions?.length}
    <div class="question-list">
      {#each survey.questions as question, index}
        <section class="question">
          <div class="question-head"><span>Q{question.number}</span>{#if question.required}<strong>必須</strong>{/if}</div>
          <div class="rich">{@html richText(question.body)}</div>
          {#if question.options?.length}
            {#if question.answer_type === "list"}
              <select value={valueFor(index)} onchange={(event) => setValue(index, (event.currentTarget as HTMLSelectElement).value)}>
                <option value="">-- 選択してください --</option>
                {#each question.options as option}<option value={option.value}>{option.label}</option>{/each}
              </select>
            {:else if question.answer_type === "checkbox"}
              <div class="option-list">
                {#each question.options as option}
                  <label><input type="checkbox" checked={isChecked(index, option.value)} onchange={(event) => toggleCheckbox(index, option.value, (event.currentTarget as HTMLInputElement).checked)} /> {option.label}</label>
                {/each}
              </div>
            {:else}
              <div class="option-list">
                {#each question.options as option}
                  <label><input type="radio" name={`survey-${index}`} checked={valueFor(index) === option.value} onchange={() => setValue(index, option.value)} /> {option.label}</label>
                {/each}
              </div>
            {/if}
          {:else if question.answer_type === "textarea"}
            <textarea placeholder="自由記述" value={valueFor(index)} oninput={(event) => setValue(index, (event.currentTarget as HTMLTextAreaElement).value)}></textarea>
          {:else}
            <input type="text" placeholder="回答を入力" value={valueFor(index)} oninput={(event) => setValue(index, (event.currentTarget as HTMLInputElement).value)} />
          {/if}
        </section>
      {/each}
    </div>
    <div class="submit-row">
      {#if status}<span class:error-status={failed || status.startsWith("未回答")}>{status}</span>{/if}
      {#if !formAvailable}<span class="error-status">フォーム情報を取得できませんでした</span>{/if}
      <button class="primary-btn" type="button" disabled={busy || submitted || !formAvailable} onclick={submit}>
        <Icon name="square.and.arrow.up" size={15} />
        <span>{busy ? "提出中..." : submitted ? "提出完了" : failed ? "再試行" : "回答を提出"}</span>
      </button>
    </div>
  {:else if !survey.description}
    <div class="empty-state">詳細情報を取得できませんでした</div>
  {/if}
</article>
