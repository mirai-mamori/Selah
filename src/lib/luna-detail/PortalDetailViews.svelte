<script lang="ts">
  import Icon from "../Icon.svelte";
  import type {
    CourseDetail,
    KwicCabinetReference,
    KwicNotificationDetail,
    MetaPair,
  } from "./types";

  interface Props {
    kwicDetail: KwicNotificationDetail | null;
    kwicCabinet: KwicCabinetReference | null;
    kgcDetail: CourseDetail | null;
    mode: string;
    titleParam: string;
    nameParam: string;
    richText: (value: string | undefined | null) => string;
    openKwicLink: (url: string, title?: string) => void | Promise<void>;
  }

  let {
    kwicDetail,
    kwicCabinet,
    kgcDetail,
    mode,
    titleParam,
    nameParam,
    richText,
    openKwicLink,
  }: Props = $props();

  const orderedFields = $derived(orderedKgcFields(kgcDetail?.fields || []));

  function standardYearIndex(fields: MetaPair[]): number {
    return fields.findIndex(([label]) => {
      const lower = String(label || "").toLowerCase();
      return label.includes("履修基準年度")
        || label.includes("履修基準")
        || lower.includes("standard year for registration")
        || lower.includes("standard year");
    });
  }

  function orderedKgcFields(fields: MetaPair[]): MetaPair[] {
    const rows = [...(fields || [])];
    const index = standardYearIndex(rows);
    if (index > 0) {
      const [row] = rows.splice(index, 1);
      rows.unshift(row);
    }
    return rows;
  }

  function normalizeDigits(value: string): string {
    return String(value || "").replace(/[０-９]/g, (digit) => String.fromCharCode(digit.charCodeAt(0) - 0xFEE0));
  }

  function standardYearBadge(value: string): string {
    const text = normalizeDigits(value);
    if (!text) return "";
    if (text.includes("全学年") || text.includes("不問")) return "1年生以上";
    const numbers = (text.match(/\d+/g) || []).map(Number).filter((number) => number >= 1 && number <= 10);
    if (!numbers.length) return "";
    const minimum = Math.min(...numbers);
    return `${minimum}年生以上`;
  }

  function containsHtmlTable(value: string): boolean {
    return /<(?:table|tbody|thead|tr|th|td)\b/i.test(String(value || ""));
  }
</script>

{#if kwicDetail}
  <article class="detail-wrap">
    <div class="course-label">KWIC ポータル</div>
    <h1>{kwicDetail.title || titleParam}</h1>
    {#if kwicDetail.date || kwicDetail.sender}
      <div class="meta-table">
        {#if kwicDetail.date}<div><span>日付</span><strong>{kwicDetail.date}</strong></div>{/if}
        {#if kwicDetail.sender}<div><span>送信者</span><strong>{kwicDetail.sender}</strong></div>{/if}
      </div>
    {/if}
    {#if kwicDetail.body_html}
      <section class="body-section">
        <div class="rich">{@html richText(kwicDetail.body_html)}</div>
      </section>
    {/if}
    {#if kwicDetail.attachments?.length}
      <div class="attachments">
        <h2>添付ファイル</h2>
        {#each kwicDetail.attachments as attachment}
          <button type="button" onclick={() => openKwicLink(attachment.url, attachment.name)}>
            <Icon name="paperclip" size={15} />
            <span>{attachment.name}</span>
            <Icon name="arrow.up.right.square" size={15} />
          </button>
        {/each}
      </div>
    {/if}
  </article>
{/if}

{#if kwicCabinet}
  <article class="detail-wrap kwic-cabinet-wrap">
    <div class="course-label">KWIC ポータル</div>
    <h1>{kwicCabinet.title || titleParam || "学生キャビネット"}</h1>
    {#if kwicCabinet.items?.length}
      <div class="cabinet-grid">
        {#each kwicCabinet.items as item}
          <button type="button" class="cabinet-card" onclick={() => openKwicLink(item.url, item.name)}>
            <strong class="cabinet-card-title">{item.name}</strong>
            {#if item.updated_at || item.is_new}
              <div class="cabinet-card-foot" class:has-badge={item.is_new}>
                {#if item.is_new}<strong class="item-badge new">NEW</strong>{/if}
                {#if item.updated_at}<small>{item.updated_at}</small>{/if}
              </div>
            {/if}
          </button>
        {/each}
      </div>
    {:else}
      <div class="state small">該当データはありません。</div>
    {/if}
  </article>
{/if}

{#if kgcDetail}
  <article class="detail-wrap">
    <div class="course-label">{mode === "syllabus" ? "シラバス" : "授業・時間割照会（詳細）"}</div>
    <h1>{nameParam || titleParam || "授業詳細"}</h1>
    {#if kgcDetail.fields?.length}
      <div class="meta-table kgc-table">
        {#each orderedFields as row, index}
          {@const isStandardYear = index === 0 && standardYearIndex(orderedFields) === 0}
          <div class:std-year-row={isStandardYear}>
            <span>{row[0]}</span>
            <div class="kgc-field-value rich" class:table-value={containsHtmlTable(row[1])}>
              {@html richText(row[1] || "-")}
              {#if isStandardYear && standardYearBadge(row[1])}
                <em class="year-range-badge">{standardYearBadge(row[1])}</em>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="state small">詳細情報を取得できませんでした。</div>
    {/if}
  </article>
{/if}

<style>
  .kwic-cabinet-wrap {
    display: grid;
    gap: 12px;
  }

  .kwic-cabinet-wrap > h1 {
    margin-bottom: 0;
  }

  .cabinet-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 6px;
  }

  .cabinet-card {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 78px;
    padding: 11px 12px;
    border: 0.5px solid var(--detail-border);
    border-radius: 10px;
    background: var(--detail-card);
    color: var(--detail-text);
    text-align: left;
    box-shadow: var(--detail-shadow);
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease, transform 0.12s ease;
  }

  .cabinet-card:hover {
    background: var(--detail-card-hover);
    border-color: var(--detail-border-soft);
    box-shadow: 0 3px 12px rgba(0,0,0,0.06);
    transform: translateY(-1px);
  }

  .cabinet-card:active {
    transform: scale(0.99);
  }

  .cabinet-card:focus-visible {
    outline: 2px solid color-mix(in srgb, var(--detail-accent, #173b68) 45%, transparent);
    outline-offset: 2px;
  }

  .cabinet-card-foot {
    display: flex;
    align-items: flex-end;
    justify-content: flex-end;
    gap: 6px;
    margin-top: auto;
    min-height: 18px;
  }

  .cabinet-card-foot.has-badge {
    justify-content: space-between;
  }

  .cabinet-card-foot small {
    color: var(--detail-muted);
    margin-left: auto;
    font-size: 10.5px;
    font-weight: 650;
    line-height: 1;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .cabinet-card-title {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    overflow: hidden;
    font-size: 13.5px;
    line-height: 1.3;
    font-weight: 780;
    color: var(--detail-text);
    word-break: break-word;
  }

  .cabinet-card .item-badge {
    padding: 1px 6px;
    font-size: 10px;
  }

  @media (max-width: 520px) {
    .cabinet-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
