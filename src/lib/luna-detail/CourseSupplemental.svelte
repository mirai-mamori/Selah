<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../Icon.svelte";
  import type { MetaPair } from "./types";

  interface TextbookEntry {
    category: string;
    author: string;
    title: string;
    publisher: string;
    year: string;
    isbn: string;
    text: string;
  }

  interface SyllabusFields {
    fields?: MetaPair[];
    textbooks?: TextbookEntry[];
  }

  interface DownloadRecord {
    filename: string;
    path: string;
    course_name: string;
    downloaded_at: number;
    file_exists: boolean;
  }

  interface Props {
    courseName: string;
    idnumberParam: string;
    richText: (value: string | undefined | null) => string;
  }

  let { courseName, idnumberParam, richText }: Props = $props();
  let textbooks = $state<TextbookEntry[]>([]);
  let fallbackTextbooks = $state<MetaPair[]>([]);
  let liveNotes = $state<DownloadRecord[]>([]);

  function normalizeCourseName(value: string): string {
    return String(value || "")
      .replace(/^.+\s\d{7,8}\s+/, "")
      .replace(/[\[［]\d+[\]］]/g, "")
      .replace(/[（(][^)）]*(?:学期|限|クラス|組|セメスター|Quarter|Semester)[^)）]*[)）]\s*$/i, "")
      .replace(/[\s_・\-–（）()]/g, "")
      .toLowerCase();
  }

  function noteMatchesCourse(note: DownloadRecord): boolean {
    if (!note.file_exists || !note.filename.includes("_live.md")) return false;
    const target = normalizeCourseName(courseName);
    const candidate = normalizeCourseName(`${note.course_name} ${note.filename} ${note.path}`);
    return !!target && (candidate.includes(target) || target.includes(normalizeCourseName(note.course_name)));
  }

  function noteDate(note: DownloadRecord): string {
    const match = note.filename.match(/^(\d{4})(\d{2})(\d{2})_/);
    if (match) {
      return `${match[1]}/${match[2]}/${match[3]}`;
    }
    const date = new Date(Number(note.downloaded_at || 0));
    if (Number.isNaN(date.getTime())) return "----/--/--";
    return `${date.getFullYear()}/${String(date.getMonth() + 1).padStart(2, "0")}/${String(date.getDate()).padStart(2, "0")}`;
  }

  async function loadSupplemental(): Promise<void> {
    const kgcCode = idnumberParam.length >= 12 ? idnumberParam.slice(4, 12) : idnumberParam;
    const [syllabus, downloads] = await Promise.all([
      kgcCode ? invoke<SyllabusFields | null>("get_kgc_syllabus_fields", { kgcCode }).catch(() => null) : Promise.resolve(null),
      courseName ? invoke<DownloadRecord[]>("list_downloads").catch(() => []) : Promise.resolve([]),
    ]);
    textbooks = syllabus?.textbooks || [];
    if (!textbooks.length) {
      fallbackTextbooks = (syllabus?.fields || []).filter(([label, value]) =>
        !!value?.trim() && /教科書|参考文献|参考書|Required texts|Reference books/i.test(label)
      );
    }
    const seen = new Set<string>();
    liveNotes = downloads
      .filter(noteMatchesCourse)
      .filter((note) => !!note.path && !seen.has(note.path) && !!seen.add(note.path))
      .sort((a, b) => Number(b.downloaded_at || 0) - Number(a.downloaded_at || 0));
  }


  onMount(() => void loadSupplemental());
</script>

{#if textbooks.length || fallbackTextbooks.length}
  <section class="section">
    <div class="section-head"><h2>教科書・参考文献</h2><span>{textbooks.length || fallbackTextbooks.length}</span></div>
    <div class="reference-list">
      {#each textbooks as textbook}
        <article>
          <strong>{textbook.title || textbook.text || textbook.category}</strong>
          {#if textbook.category}<span class="category">{textbook.category.replace(/Required texts|Reference books/gi, "").replace(/・資料等?/g, "").trim()}</span>{/if}
          {#if textbook.author || textbook.publisher || textbook.year}
            <span>{[textbook.author, textbook.publisher, textbook.year].filter(Boolean).join(" / ")}</span>
          {/if}
          {#if textbook.isbn}<small>ISBN: {textbook.isbn}</small>{/if}
        </article>
      {/each}
      {#each fallbackTextbooks as field}
        <article>
          <strong>{field[0].replace(/Required texts|Reference books/gi, "").replace(/・資料等?/g, "").trim()}</strong>
          <div class="rich compact">{@html richText(field[1])}</div>
        </article>
      {/each}
    </div>
  </section>
{/if}

{#if liveNotes.length}
  <section class="section">
    <div class="section-head"><h2>LIVE ノート</h2><span>{liveNotes.length}</span></div>
    <div class="note-grid">
      {#each liveNotes as note (note.path)}
        <button type="button" aria-label={`${noteDate(note)} の LIVE ノートを開く`} onclick={() => invoke("open_downloaded_file", { path: note.path })}>
          <span><strong class="live-note-date">{noteDate(note)}</strong></span>
          <Icon name="arrow.up.right.square" size={15} />
        </button>
      {/each}
    </div>
  </section>
{/if}


<style>
  .reference-list,
  .note-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }

  .reference-list article,
  .note-grid button {
    min-width: 0;
    display: flex;
    gap: 8px;
    padding: 12px;
    border: 0.5px solid rgba(0,0,0,0.08);
    border-radius: 8px;
    background: var(--detail-card);
    color: var(--detail-text);
    box-shadow: var(--detail-shadow);
    text-align: left;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease, transform 0.1s ease;
  }

  .reference-list article {
    flex-direction: column;
  }

  .reference-list span,
  .reference-list small {
    color: var(--detail-muted);
    font-size: 11.5px;
  }

  .reference-list .category {
    color: var(--detail-accent);
    font-weight: 750;
  }

  .note-grid button {
    align-items: center;
    justify-content: space-between;
    cursor: pointer;
  }

  .note-grid button:hover {
    background: var(--detail-card-hover);
    border-color: var(--detail-border-soft);
    box-shadow: 0 2px 8px rgba(0,0,0,0.055);
  }

  .note-grid button:active {
    transform: scale(0.99);
  }

  .note-grid button span {
    min-width: 0;
    display: grid;
    gap: 3px;
  }

  .note-grid strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .note-grid .live-note-date {
    font-size: 14px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
  }


  /* Same unified responsive logic as the other detail grids (3 → 2 → 1). */
  @media (max-width: 640px) {
    .reference-list,
    .note-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 420px) {
    .reference-list,
    .note-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
