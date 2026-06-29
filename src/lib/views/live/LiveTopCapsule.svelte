<script lang="ts">
  import Icon from "../../Icon.svelte";
  import type { IconName } from "../../Icon.svelte";
  import type { CourseSlot } from "../../schedule";
  import type { LiveControlModel, NoticeState } from "./liveTypes";

  interface Props {
    control: LiveControlModel;
    notice: NoticeState;
    renderedCourseOptions: CourseSlot[];
    selectedKey: string;
    pageLoading: boolean;
    busy: boolean;
    canStop: boolean;
    canGenerateOverallSummary: boolean;
    confirmClear: boolean;
    freeNoteKey: string;
    courseKey: (course: CourseSlot) => string;
    courseLabel: (course: CourseSlot) => string;
    onStart: () => void;
    onClearCourseData: () => void;
    onCancelClear: () => void;
    onConfirmClear: () => void;
    onStopLive: () => void;
    onGenerateOverallSummary: () => void;
    onPauseLive: () => void;
    onResumeLive: () => void;
    onOpenAiSettings: () => void;
  }

  let {
    control,
    notice,
    renderedCourseOptions,
    selectedKey = $bindable(),
    pageLoading,
    busy,
    canStop,
    canGenerateOverallSummary,
    confirmClear,
    freeNoteKey,
    courseKey,
    courseLabel,
    onStart,
    onClearCourseData,
    onCancelClear,
    onConfirmClear,
    onStopLive,
    onGenerateOverallSummary,
    onPauseLive,
    onResumeLive,
    onOpenAiSettings,
  }: Props = $props();

  let courseMenuOpen = $state(false);

  const primaryIcon = $derived.by((): IconName => {
    switch (control.primaryAction) {
      case "start":
        return "microphone";
      case "pause":
        return "pause";
      case "resume":
        return "play";
      case "settings":
        return "gear";
      default:
        return "pulse";
    }
  });

  const triggerIcon = $derived.by((): IconName => {
    if (control.showSaveAction) return "save";
    return primaryIcon;
  });

  const triggerLabel = $derived.by(() => {
    if (control.showSaveAction) return "終了";
    return control.primaryLabel;
  });

  const triggerDisabled = $derived.by(() => {
    if (control.showSaveAction) return !canStop;
    return control.primaryDisabled;
  });

  const triggerTitle = $derived.by(() => {
    if (control.showSaveAction) return "保存して終了";
    return control.primaryTitle;
  });

  const runningLayout = $derived(
    control.phase === "recording" ||
      control.phase === "paused" ||
      control.phase === "booting" ||
      control.phase === "thinking",
  );

  const selectedCourse = $derived.by(() => {
    if (selectedKey === freeNoteKey) return null;
    return renderedCourseOptions.find((course) => courseKey(course) === selectedKey) ?? null;
  });

  const selectedCourseName = $derived(
    selectedKey === freeNoteKey ? "自由ノート" : selectedCourse?.name ?? control.targetLabel,
  );

  function courseMeta(course: CourseSlot): string {
    const label = courseLabel(course);
    const match = label.match(/\((.*)\)$/);
    return match?.[1] ?? label.replace(course.name, "").trim();
  }

  $effect(() => {
    control.phase;
    if (!control.showModeSelect) courseMenuOpen = false;
  });

  function closePanel() {
    courseMenuOpen = false;
    if (confirmClear) onCancelClear();
  }

  function closeCourseMenu() {
    courseMenuOpen = false;
    if (confirmClear) onCancelClear();
  }

  function toggleCourseMenu() {
    if (courseMenuOpen) closeCourseMenu();
    else courseMenuOpen = true;
  }

  function chooseTarget(key: string) {
    selectedKey = key;
    closeCourseMenu();
  }

  function runPrimaryAction() {
    switch (control.primaryAction) {
      case "start":
        closePanel();
        onStart();
        break;
      case "pause":
        closePanel();
        onPauseLive();
        break;
      case "resume":
        closePanel();
        onResumeLive();
        break;
      case "settings":
        openAiSettings();
        break;
    }
  }

  function runSummary() {
    closePanel();
    onGenerateOverallSummary();
  }

  function openAiSettings() {
    closePanel();
    onOpenAiSettings();
  }

  function runTriggerAction() {
    if (control.showSaveAction) {
      runStop();
      return;
    }
    runPrimaryAction();
  }

  function runStop() {
    closePanel();
    onStopLive();
  }

  function confirmClearAndClose() {
    closePanel();
    onConfirmClear();
  }
</script>

<header class="top-capsule" class:running={runningLayout} data-phase={control.phase} data-tone={control.tone}>
  <div class="capsule-shell">
    {#if runningLayout}
      <div class="running-panel">
        <div class="running-meta">
          <span class="running-course">{control.targetLabel}</span>
          <span class="running-context">
            <span class="live-dot" aria-hidden="true"></span>
            <span class="status-label">{control.statusLabel}</span>
            {#if control.targetMeta}
              <span class="running-divider" aria-hidden="true"></span>
              <span>{control.targetMeta}</span>
            {/if}
          </span>
        </div>

        <div class="running-actions" role="toolbar" aria-label="LIVE 操作">
          {#if control.primaryAction === "pause" || control.primaryAction === "resume" || control.primaryAction === "settings"}
            <button
              class="operation-action"
              type="button"
              onclick={runPrimaryAction}
              disabled={control.primaryDisabled}
              title={control.primaryTitle}
            >
              <Icon name={primaryIcon} size={14} />
              <span>{control.primaryLabel}</span>
            </button>
          {/if}

          {#if control.showSummaryAction}
            <button class="operation-action" type="button" onclick={runSummary} disabled={!canGenerateOverallSummary} title="現在までの全体要約">
              <Icon name="sparkles" size={14} />
              <span>全体要約</span>
            </button>
          {/if}

          {#if control.showSaveAction}
            <button class="operation-action finish" type="button" onclick={runStop} disabled={!canStop} title="保存して終了">
              <Icon name="save" size={14} />
              <span>終了</span>
            </button>
          {/if}

          {#if control.primaryAction === "none" && control.saveSteps.length > 0}
            {@const total = control.saveSteps.length}
            {@const idx = control.saveStepIndex}
            {@const nextLabel = control.saveSteps[idx + 1]?.replace(/中$/, "")}
            <div class="save-stepper" role="status" aria-live="polite">
              <div class="save-stepper-head">
                <span class="mini-spinner" aria-hidden="true"></span>
                <span class="save-stepper-label">{control.saveSteps[idx]}…</span>
                {#if total > 1}
                  <span class="save-stepper-count">{idx + 1}/{total}</span>
                {/if}
              </div>
              {#if total > 1}
                <!-- Determinate bar only for multi-step pipelines, where the fill
                     reflects real pipeline position. A single step has no progress
                     to show, so the spinner + label stand alone. -->
                <div class="save-stepper-track" aria-hidden="true">
                  <span class="save-stepper-fill" style="width: {((idx + 1) / total) * 100}%"></span>
                </div>
              {/if}
              {#if nextLabel}
                <span class="save-stepper-next">次: {nextLabel}</span>
              {/if}
            </div>
          {:else if control.primaryAction === "none" && (control.progressLabel || control.detailLabel)}
            <span class="operation-progress" role="status">
              <span class="mini-spinner" aria-hidden="true"></span>
              <span>{control.progressLabel || control.detailLabel}</span>
            </span>
          {/if}
        </div>
      </div>
    {:else}
      <div class="capsule-bar">
        <div class="target-cluster">
          {#if control.phase === "saved"}
            <span class="bar-status" role="status">
              <span class="live-dot" aria-hidden="true"></span>
              <span class="status-label">{control.statusLabel}</span>
            </span>
          {/if}
          {#if control.showModeSelect}
            <div class="target-picker" class:open={courseMenuOpen}>
              <button
                class="target-trigger"
                type="button"
                disabled={pageLoading}
                aria-haspopup="listbox"
                aria-expanded={courseMenuOpen}
                onclick={toggleCourseMenu}
              >
                <span class="target-trigger-title">{selectedCourseName}</span>
                <span class="target-caret" aria-hidden="true"></span>
              </button>
              {#if courseMenuOpen}
                <div class="course-menu" role="listbox" aria-label="録音対象">
                  <div class="course-row" class:selected={selectedKey === freeNoteKey}>
                    <button
                      class="course-option"
                      type="button"
                      role="option"
                      aria-selected={selectedKey === freeNoteKey}
                      onclick={() => chooseTarget(freeNoteKey)}
                    >
                      <span class="course-option-title">自由ノート</span>
                      <span class="course-option-meta">自由入力</span>
                    </button>
                  </div>
                  {#if renderedCourseOptions.length > 0}
                    <div class="course-menu-group">授業</div>
                    {#each renderedCourseOptions as course}
                      {@const key = courseKey(course)}
                      {@const showClear = selectedKey === key && control.showClearAction}
                      <div class="course-row" class:selected={selectedKey === key}>
                        <button
                          class="course-option"
                          type="button"
                          role="option"
                          aria-selected={selectedKey === key}
                          onclick={() => chooseTarget(key)}
                        >
                          <span class="course-option-title">{course.name}</span>
                          {#if !(showClear && confirmClear)}
                            <span class="course-option-meta">{courseMeta(course)}</span>
                          {/if}
                        </button>
                        {#if showClear}
                          <button
                            class="row-clear"
                            class:armed={confirmClear}
                            type="button"
                            onclick={confirmClear ? confirmClearAndClose : onClearCourseData}
                            disabled={busy}
                            title={confirmClear ? "もう一度クリックで削除" : "クリックを2回で記録をクリア"}
                            aria-label={confirmClear ? "もう一度クリックで削除" : "記録をクリア"}
                          >
                            <Icon name="trash" size={13} />
                            {#if confirmClear}<span>削除</span>{/if}
                          </button>
                        {/if}
                      </div>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {:else}
            <span class="target-title">{control.targetLabel}</span>
          {/if}
        </div>

        <div class="capsule-actions">
          {#if control.primaryAction !== "none"}
            <button
              class="capsule-action primary"
              type="button"
              onclick={runTriggerAction}
              disabled={triggerDisabled}
              title={triggerTitle}
            >
              <Icon name={triggerIcon} size={14} />
              <span>{triggerLabel}</span>
            </button>
          {:else}
            <span class="capsule-action progress" role="status">
              <span class="mini-spinner" aria-hidden="true"></span>
              <span>{control.progressLabel || control.detailLabel}</span>
            </span>
          {/if}
        </div>
      </div>
    {/if}

    {#if notice}
      <div class="capsule-notice" data-kind={notice.kind} role="status">
        <span class="capsule-notice-dot" aria-hidden="true"></span>
        <span class="capsule-notice-text" title={notice.text}>{notice.text}</span>
        {#if notice.action === "open-ai-settings"}
          <button class="capsule-notice-action" type="button" onclick={onOpenAiSettings}>AI設定</button>
        {/if}
      </div>
    {/if}
  </div>
</header>

<style>
  .top-capsule {
    position: absolute;
    /* Insets ≥ the shadow blur (~18px) so the left/top shadow isn't clipped by
       .live-root's overflow:hidden now that the island is left-aligned. */
    top: 16px;
    left: 18px;
    z-index: 40;
    width: max-content;
    max-width: min(860px, calc(100% - 36px));
    /* Neutral-button accent fill. Light accent is dark navy (needs a stronger
       tint to read); dark accent is vivid gold (needs a lighter tint). */
    --live-btn-fill: color-mix(in srgb, var(--accent) 13%, var(--bg-input));
    --live-btn-fill-hover: color-mix(in srgb, var(--accent) 20%, var(--bg-input));
  }

  :global([data-theme="dark"]) .top-capsule {
    --live-btn-fill: color-mix(in srgb, var(--accent) 5%, var(--bg-input));
    --live-btn-fill-hover: color-mix(in srgb, var(--accent) 11%, var(--bg-input));
  }

  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) .top-capsule {
      --live-btn-fill: color-mix(in srgb, var(--accent) 5%, var(--bg-input));
      --live-btn-fill-hover: color-mix(in srgb, var(--accent) 11%, var(--bg-input));
    }
  }

  .top-capsule.running {
    width: max-content;
    max-width: min(560px, calc(100% - 36px));
  }

  .capsule-shell {
    width: 100%;
    border-radius: 22px;
    background: color-mix(in srgb, var(--glass-bg) 58%, var(--bg-primary));
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-glass), 0 6px 18px rgba(0, 0, 0, 0.08);
    backdrop-filter: var(--glass-blur) var(--glass-saturate);
    -webkit-backdrop-filter: var(--glass-blur) var(--glass-saturate);
    overflow: visible;
  }

  /* Transient notices live inside the island now (no separate banner). A thin
     row under the main content, tinted by kind. */
  .capsule-notice {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    min-width: 220px;
    max-width: min(420px, calc(100vw - 48px));
    padding: 7px 12px 8px;
    border-top: 0.5px solid var(--glass-border);
    color: var(--tone, var(--text-secondary));
    font-size: 11.5px;
    font-weight: 750;
    line-height: 1.35;
    animation: island-in 0.18s cubic-bezier(0.22, 1, 0.36, 1) both;
  }
  .capsule-notice[data-kind="error"] { --tone: var(--red); }
  .capsule-notice[data-kind="warning"] { --tone: var(--orange, #e67700); }
  .capsule-notice[data-kind="success"] { --tone: var(--green); }
  .capsule-notice-dot {
    flex: 0 0 auto;
    width: 6px;
    height: 6px;
    margin-top: 4px;
    border-radius: 50%;
    background: var(--tone, currentColor);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--tone, currentColor) 24%, transparent);
  }
  .capsule-notice-text {
    min-width: 0;
    flex: 1 1 auto;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .capsule-notice-action {
    flex: 0 0 auto;
    border: 0;
    border-radius: 999px;
    padding: 3px 10px;
    background: color-mix(in srgb, var(--tone, currentColor) 14%, transparent);
    color: var(--tone, currentColor);
    font-family: inherit;
    font-size: 11px;
    font-weight: 800;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .capsule-notice-action:hover {
    background: color-mix(in srgb, var(--tone, currentColor) 22%, transparent);
  }

  .capsule-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 42px;
    padding: 5px 6px 5px 8px;
  }

  .live-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.72;
    box-shadow: 0 0 0 1px color-mix(in srgb, currentColor 22%, transparent);
    position: relative;
    flex: 0 0 auto;
  }

  .top-capsule[data-tone="recording"] .live-dot {
    background: var(--red);
    color: var(--red);
    opacity: 1;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--red) 24%, transparent);
  }

  .top-capsule[data-tone="recording"] .live-dot::after {
    content: "";
    position: absolute;
    inset: 50%;
    width: 100%;
    height: 100%;
    border-radius: inherit;
    background: currentColor;
    opacity: 0.28;
    transform: translate(-50%, -50%) scale(1);
    animation: pulse-dot 1.25s ease-in-out infinite;
    pointer-events: none;
  }

  .top-capsule[data-tone="paused"] .live-dot {
    background: var(--blue);
    color: var(--blue);
    opacity: 1;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--blue) 24%, transparent);
  }

  .top-capsule[data-tone="saved"] .live-dot {
    background: var(--green);
    color: var(--green);
    opacity: 1;
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--green) 24%, transparent);
  }

  .top-capsule[data-tone="paused"] .status-label {
    color: var(--blue);
  }

  .top-capsule[data-tone="saved"] .status-label {
    color: var(--green);
  }

  .bar-status {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding-left: 4px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0;
    white-space: nowrap;
  }

  .running-panel {
    display: grid;
    grid-template-rows: auto auto;
    gap: 5px;
    padding: 7px 8px 8px;
    contain: layout paint;
    animation: island-in 0.18s cubic-bezier(0.22, 1, 0.36, 1) both;
  }

  .running-meta {
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 8px;
    padding-inline: 4px;
  }

  .running-course {
    min-width: 0;
    max-width: 210px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 850;
    letter-spacing: 0;
  }

  .running-context {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    overflow: hidden;
    color: var(--text-tertiary);
    font-size: 10.5px;
    font-weight: 750;
    line-height: 1;
    white-space: nowrap;
  }

  .running-context .live-dot {
    width: 6px;
    height: 6px;
  }

  .running-divider {
    width: 1px;
    height: 10px;
    flex: 0 0 auto;
    background: color-mix(in srgb, var(--text-primary) 12%, transparent);
  }

  .running-actions {
    display: flex;
    flex-wrap: nowrap;
    gap: 6px;
    align-items: center;
    justify-content: flex-start;
  }

  .target-cluster {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 auto;
  }

  .target-title {
    min-width: 0;
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 700;
    letter-spacing: 0;
  }

  .target-picker {
    position: relative;
    min-width: 0;
    max-width: 300px;
    flex: 1 1 auto;
  }

  .target-trigger {
    width: 100%;
    min-height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 9px 6px 10px;
    border-radius: 12px;
    border: 0.5px solid var(--border-strong);
    background: var(--live-btn-fill);
    color: var(--text-primary);
    box-shadow: var(--glass-highlight), 0 1px 2px rgba(0, 0, 0, 0.04);
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 800;
    letter-spacing: 0;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
  }

  .target-trigger:hover:not(:disabled),
  .target-picker.open .target-trigger {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border-strong));
    background: var(--live-btn-fill-hover);
  }

  .target-trigger:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 16%, transparent);
  }

  .target-trigger:disabled {
    cursor: default;
    opacity: 0.52;
  }

  .target-trigger-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .target-caret {
    width: 7px;
    height: 7px;
    flex: 0 0 auto;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    transform: translateY(-2px) rotate(45deg);
    opacity: 0.58;
  }

  .target-picker.open .target-caret {
    transform: translateY(2px) rotate(225deg);
  }

  .course-menu {
    position: absolute;
    top: calc(100% + 7px);
    left: 0;
    z-index: 70;
    width: min(360px, calc(100vw - 32px));
    max-height: 320px;
    overflow-y: auto;
    padding: 6px;
    border-radius: 14px;
    border: 0.5px solid var(--glass-border);
    background: color-mix(in srgb, var(--glass-bg) 50%, var(--bg-primary));
    box-shadow: var(--shadow-glass), 0 6px 18px rgba(0, 0, 0, 0.08);
    backdrop-filter: var(--glass-blur) var(--glass-saturate);
    -webkit-backdrop-filter: var(--glass-blur) var(--glass-saturate);
    animation: menu-in 0.16s cubic-bezier(0.22, 1, 0.36, 1) both;
  }

  .course-menu-group {
    padding: 7px 8px 4px;
    color: var(--text-tertiary);
    font-size: 10.5px;
    font-weight: 800;
    letter-spacing: 0;
  }

  .course-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 4px;
    border-radius: 10px;
  }

  .course-row:hover,
  .course-row.selected {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
  }

  .course-row.selected .course-option-title {
    color: color-mix(in srgb, var(--accent) 82%, var(--text-primary));
  }

  .course-option {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 9px;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--text-primary);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row-clear {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    height: 26px;
    min-width: 26px;
    padding: 0 6px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: color-mix(in srgb, var(--red) 68%, var(--text-tertiary));
    font-family: inherit;
    font-size: 11.5px;
    font-weight: 800;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease;
  }

  .row-clear:hover:not(:disabled) {
    background: color-mix(in srgb, var(--red) 12%, transparent);
    color: var(--red);
  }

  .row-clear.armed {
    background: color-mix(in srgb, var(--red) 16%, transparent);
    color: var(--red);
  }

  .row-clear.armed:hover:not(:disabled) {
    background: color-mix(in srgb, var(--red) 22%, transparent);
  }

  .row-clear:disabled {
    cursor: default;
    opacity: 0.42;
  }

  .course-option-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 800;
    letter-spacing: 0;
  }

  .course-option-meta {
    flex: 0 0 auto;
    max-width: 42%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .capsule-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 5px;
    flex: 0 0 auto;
  }

  .capsule-action,
  .operation-action {
    font-family: inherit;
  }

  .capsule-action {
    min-height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 6px 11px;
    border: 0;
    border-radius: 13px;
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0;
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
    transition: background 0.15s ease, color 0.15s ease, transform 0.12s ease, opacity 0.15s ease;
  }

  .capsule-action:active,
  .operation-action:active {
    transform: scale(0.97);
  }

  .capsule-action:disabled,
  .operation-action:disabled {
    cursor: default;
    opacity: 0.42;
    transform: none;
  }

  .capsule-action.primary {
    min-width: 78px;
    background: var(--accent);
    color: var(--text-on-accent);
    box-shadow: 0 4px 14px color-mix(in srgb, var(--accent) 24%, transparent);
  }

  .capsule-action.primary:hover:not(:disabled) {
    background: var(--accent-hover, color-mix(in srgb, var(--accent) 84%, #000));
  }

  .capsule-action.progress {
    cursor: default;
    background: color-mix(in srgb, var(--accent) 9%, transparent);
    color: color-mix(in srgb, var(--accent) 80%, var(--text-primary));
  }

  .operation-progress {
    min-height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    justify-content: center;
    min-width: 0;
    padding: 5px 9px;
    border: 0.5px solid color-mix(in srgb, var(--text-primary) 9%, transparent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--text-primary) 6%, transparent);
    color: color-mix(in srgb, var(--accent) 78%, var(--text-primary));
    font-size: 11.5px;
    font-weight: 850;
    line-height: 1;
    white-space: nowrap;
  }

  .save-stepper {
    flex: 1 1 auto;
    min-width: 200px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 4px 4px 2px;
  }

  .save-stepper-head {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }

  .save-stepper-label {
    min-width: 0;
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: color-mix(in srgb, var(--accent) 78%, var(--text-primary));
    font-size: 12px;
    font-weight: 850;
    line-height: 1;
  }

  .save-stepper-count {
    flex: 0 0 auto;
    color: var(--text-tertiary);
    font-size: 10.5px;
    font-weight: 800;
    font-variant-numeric: tabular-nums;
  }

  .save-stepper-track {
    height: 3px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
    overflow: hidden;
  }

  .save-stepper-fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 0.35s cubic-bezier(0.22, 1, 0.36, 1);
  }

  .save-stepper-next {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-tertiary);
    font-size: 10.5px;
    font-weight: 750;
    line-height: 1;
  }

  .operation-action {
    min-height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    min-width: 0;
    padding: 6px 10px;
    border: 0.5px solid var(--border-strong);
    border-radius: 12px;
    background: var(--live-btn-fill);
    color: var(--text-primary);
    box-shadow: var(--glass-highlight), 0 1px 2px rgba(0, 0, 0, 0.04);
    font-size: 12px;
    font-weight: 850;
    letter-spacing: 0;
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease, color 0.15s ease, transform 0.12s ease, opacity 0.15s ease;
  }

  .operation-action span,
  .operation-progress span:last-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Neutral hover for variant-less action buttons (pause / resume / settings / 全体要約). */
  .operation-action:not(.finish):hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 30%, var(--border-strong));
    background: var(--live-btn-fill-hover);
  }

  .operation-action.finish {
    border-color: color-mix(in srgb, var(--red) 22%, var(--border-strong));
    background: color-mix(in srgb, var(--red) 10%, var(--bg-input));
    color: color-mix(in srgb, var(--red) 72%, var(--text-secondary));
  }

  .operation-action.finish:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--red) 34%, var(--border-strong));
    background: color-mix(in srgb, var(--red) 16%, var(--bg-input));
    color: color-mix(in srgb, var(--red) 82%, var(--text-secondary));
  }

  .mini-spinner {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    border: 1.8px solid color-mix(in srgb, currentColor 26%, transparent);
    border-top-color: currentColor;
    animation: spin 0.8s linear infinite;
  }

  @keyframes pulse-dot {
    0% {
      opacity: 0.32;
      transform: translate(-50%, -50%) scale(1);
    }
    100% {
      opacity: 0;
      transform: translate(-50%, -50%) scale(2.25);
    }
  }

  @keyframes island-in {
    from {
      opacity: 0;
      transform: translateY(-4px) scale(0.99);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(-5px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  @media (prefers-reduced-motion: reduce) {
    .top-capsule[data-tone="recording"] .live-dot,
    .course-menu,
    .running-panel,
    .mini-spinner {
      animation: none;
    }

    .target-trigger,
    .capsule-action,
    .operation-action,
    .save-stepper-fill {
      transition: none;
    }
  }

  @media (max-width: 720px) {
    .top-capsule {
      left: 12px;
      right: 12px;
      width: auto;
      max-width: none;
      transform: none;
    }

    .capsule-bar {
      flex-wrap: wrap;
      gap: 6px;
      padding: 6px;
    }

    .target-cluster {
      flex: 1 1 calc(100% - 112px);
      order: 1;
    }

    .target-title,
    .target-picker {
      max-width: none;
      flex: 1 1 auto;
    }

    .course-menu {
      width: min(360px, calc(100vw - 24px));
      left: 0;
    }

    .capsule-actions {
      width: 100%;
      order: 2;
    }

    .capsule-action.primary {
      flex: 1 1 auto;
      min-width: 0;
    }

    .running-panel {
      padding: 7px;
    }

    .running-meta {
      flex-wrap: wrap;
      gap: 5px 8px;
    }

    .running-actions {
      flex-wrap: wrap;
    }

    .operation-action,
    .operation-progress {
      flex: 0 0 auto;
    }
  }
</style>
