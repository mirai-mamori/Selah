<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Icon from "../Icon.svelte";
  import { applyAuxiliaryTheme, syncAuxiliaryTheme } from "../auxiliarySurfaceTheme";

  interface MemoryItem {
    topic: string;
    courseName: string;
    at: number;
  }
  interface DetectiveMemory {
    mastered: MemoryItem[];
    mistakes: MemoryItem[];
    recentEvidenceTitles: string[];
    updatedAt: number;
  }
  interface CampaignCharacter {
    name: string;
    role: string;
    bond: string;
    background: string;
    motivation: string;
    stake: string;
  }
  interface CampaignRevelation {
    stage: number;
    threshold: number;
    title: string;
    reveal: string;
    unlocked: boolean;
  }
  interface DetectiveCampaign {
    courseKey: string;
    courseName: string;
    worldLabel: string;
    setting: string;
    tagline: string;
    cast: CampaignCharacter[];
    metaMystery: string;
    metaProgress: number;
    metaArc: CampaignRevelation[];
    finale: string;
    chapters: { id: string; title: string; summary: string; playedAt: number }[];
  }
  interface DetectiveContext {
    courses: DetectiveCourse[];
    reviewQueue: DetectiveReviewItem[];
    recentResults: DetectiveCaseResult[];
    generatedAt: number;
    includedCourseKeys: string[];
    memory: DetectiveMemory;
    campaigns: DetectiveCampaign[];
  }

  interface DetectiveCourse {
    name: string;
    key: string;
    liveRecords: DetectiveLiveRecord[];
    examSignals: DetectiveSignal[];
    latestAt: number;
    doubts: DetectiveDoubt[];
    recentResults: DetectiveCaseResult[];
    caseType: string;
    readiness: string;
  }

  interface DetectiveLiveRecord {
    id: string;
    filename: string;
    path: string;
    courseName: string;
    downloadedAt: number;
    excerpt: string;
  }

  interface DetectiveSignal {
    id: string;
    sourceId: string;
    title: string;
    date: string;
    category: string;
    source: string;
    courseInfo: string;
    sourceUrl: string;
    informationType: string;
    personCategoryCd: string;
    categoryCd: string;
  }

  interface DetectiveEvidence {
    id: string;
    sourceId: string;
    sourceType: "live" | "signal" | "doubt";
    source: string;
    title: string;
    date: string;
    excerpt: string;
    sourcePath: string;
    sourceUrl: string;
    informationType: string;
    personCategoryCd: string;
    categoryCd: string;
  }

  interface DetectiveTestimony {
    id: string;
    text: string;
    isFalse: boolean;
    keyEvidenceId: string;
    highlights: string[];
    pressResponse: string;
  }

  interface DetectiveAct {
    id: string;
    index: number;
    kind: "investigation" | "testimony";
    title: string;
    location: string;
    narrative: string;
    seedsMeta: boolean;
    evidenceIds: string[];
    witnessName: string;
    witnessRole: string;
    testimony: DetectiveTestimony[];
  }

  interface KnowledgePoint {
    id: string;
    label: string;
    gist: string;
    mustCover: boolean;
  }
  interface CoverageEntry {
    pointId: string;
    placement: string;
  }
  interface DetectiveCase {
    id: string;
    courseKey: string;
    courseName: string;
    title: string;
    caseType: string;
    difficulty: number;
    briefing: string;
    evidence: DetectiveEvidence[];
    finalQuestion: string;
    testimony: DetectiveTestimony[];
    scenario: string;
    witnessName: string;
    witnessRole: string;
    generationMode: string;
    generationNote: string;
    sessionNum: number;
    knowledgePoints: KnowledgePoint[];
    coverage: CoverageEntry[];
    acts: DetectiveAct[];
    caseLogic: CaseLogic;
    metaBeat: string;
  }

  interface CaseLogic {
    truth: string;
    culprit: string;
    motive: string;
    redHerrings: string[];
    deductionChain: string[];
  }

  interface DetectiveDoubt {
    id: string;
    courseName: string;
    note: string;
    evidenceIds: string[];
    createdAt: number;
    dueAt: number;
  }

  interface DetectiveCaseResult {
    id: string;
    caseId: string;
    courseKey: string;
    courseName: string;
    caseTitle: string;
    caseType: string;
    selectedEvidenceIds: string[];
    relation: string;
    deduction: string;
    closedAt: number;
    confidence: number;
  }

  interface DetectiveReviewItem {
    id: string;
    courseKey: string;
    courseName: string;
    reason: string;
    priority: number;
    dueAt: number;
  }

  interface DetectiveChapterInfo {
    liveId: string;
    index: number;
    title: string;
    generated: boolean;
    played: boolean;
    bestConfidence: number;
    playedAt: number;
    locked: boolean;
    aligned: boolean;
  }

  type Scene =
    | "list"
    | "chapters"       // chapter list for the focused course campaign
    | "loading"
    | "ai_error"
    | "act_intro"      // story card: the current act's title + narrative beat
    | "investigate"    // investigation act: read the act's evidence cards
    | "cross_exam"     // testimony act: single persistent layout, sub-state in xeMode
    | "act_clear"      // brief 異議あり cut-in between acts (auto-advances)
    | "result_pass"    // chapter-clear 異議あり result
    | "result_fail"    // brief ダメだ overlay (auto-recovers)
    | "unresolved"
    | "closing"
    | "closed";

  /// Sub-state inside the cross_exam scene. Press and Present are NOT separate
  /// scenes — they mutate the same layout. press expands the testimony card
  /// inline; presenting opens the Court Record overlay over the right half.
  type XeMode = "idle" | "pressed" | "presenting";

  const MAX_LIVES = 3;

  let loading = $state(true);
  let error = $state("");
  let context = $state<DetectiveContext>({
    courses: [],
    reviewQueue: [],
    recentResults: [],
    generatedAt: 0,
    includedCourseKeys: [],
    memory: { mastered: [], mistakes: [], recentEvidenceTitles: [], updatedAt: 0 },
    campaigns: [],
  });
  let currentCase = $state<DetectiveCase | null>(null);
  let selectedCourseKey = $state("");
  /// Course keys the user has opted into. Empty by default → selection screen shows.
  let includedKeys = $state<Set<string>>(new Set());
  /// True when the picker is in "edit selection" mode (re-opened after first run).
  let editingSelection = $state(false);
  let courseQuery = $state("");

  let scene = $state<Scene>("list");
  /// The course whose campaign chapter-list is currently in focus.
  let campaignCourseKey = $state("");
  /// Chapters of the focused campaign (one per Live note).
  let chapters = $state<DetectiveChapterInfo[]>([]);
  let chaptersLoading = $state(false);
  /// The Live note id of the chapter currently being played (for retry).
  let currentLiveId = $state("");
  /// Count of meta-arc reveals unlocked when the current chapter began — used
  /// to surface NEWLY unlocked truths on the chapter-clear screen.
  let revealedAtStart = $state(0);
  /// Reveals unlocked by finishing the current chapter (shown on close).
  let newRevelations = $state<CampaignRevelation[]>([]);
  /// meta_progress when the current chapter began — to detect campaign completion.
  let progressAtStart = $state(0);
  /// True when finishing the current chapter pushed the campaign to 100%.
  let campaignJustCompleted = $state(false);
  /// Regenerating the campaign world (force rebuild).
  let regenerating = $state(false);
  /// Which act (幕) of the chapter the player is currently in.
  let actIndex = $state(0);
  /// Evidence carousel position during investigate.
  let evidenceIndex = $state(0);
  /// Statement cursor during cross_exam.
  let xeIndex = $state(0);
  /// Cross-exam sub-state. Lives in cross_exam, doesn't change the scene.
  let xeMode = $state<XeMode>("idle");
  /// Evidence picked while xeMode === "presenting" (Court Record overlay).
  let presentedEvidenceId = $state("");
  /// Lies the player has already busted in the current session.
  let bustedLieIds = $state<Set<string>>(new Set());
  let lives = $state(MAX_LIVES);
  let shakeKey = $state(0);
  /// Cycling label for the multi-pass (outline → draft → editor) generation.
  let loadingStage = $state(0);
  const LOADING_STAGES = [
    "推理プロットを構成中…",
    "脚本を執筆中…",
    "校閲して仕上げ中…",
  ];
  let lastClosedTitle = $state("");
  let resolvedOk = $state(false);

  let courses = $derived(context.courses);
  /// HQ stats — solved cases, busted lies, pending doubts.
  let memory = $derived.by(() => context.memory ?? { mastered: [], mistakes: [], recentEvidenceTitles: [], updatedAt: 0 });
  let stat_resolved = $derived.by(
    () => context.recentResults.filter((r) => r.confidence >= 4).length,
  );
  let stat_busted = $derived.by(() => memory.mastered.length);
  let stat_pending = $derived.by(() => memory.mistakes.length);
  /// Show the most recent N mistakes for the home panel.
  let pendingTopics = $derived.by(() => memory.mistakes.slice(-5).reverse());
  /// A course can be played only if it has lecture material to mine.
  let courseHasMaterial = (c: DetectiveCourse) =>
    c.liveRecords.length + c.examSignals.length > 0;
  /// Courses the user can actually pick (have material).
  let selectableCourses = $derived.by(() => courses.filter(courseHasMaterial));
  /// True when every selectable course is already opted in.
  let allSelected = $derived.by(
    () => selectableCourses.length > 0 && selectableCourses.every((c) => includedKeys.has(c.key)),
  );
  /// Courses the user has opted into (i.e. the active Detective roster).
  let activeCourses = $derived.by(() => courses.filter((c) => includedKeys.has(c.key)));
  /// True when the user has opted into at least one course.
  let hasSelection = $derived.by(() => activeCourses.length > 0);
  /// The course whose campaign is in focus (chapter list / title banner).
  /// Falls back to the first active course.
  let campaignCourse = $derived.by(
    () =>
      activeCourses.find((c) => c.key === campaignCourseKey) ?? activeCourses[0] ?? null,
  );
  /// The campaign world for the focused course, if its bible exists.
  let activeCampaign = $derived.by(() => {
    if (!campaignCourse) return null;
    return (context.campaigns ?? []).find((c) => c.courseKey === campaignCourse.key) ?? null;
  });
  /// Rows shown in the selection modal — all courses, filtered by query.
  let visibleCourses = $derived.by(() => {
    const query = courseQuery.trim().toLocaleLowerCase();
    if (!query) return courses;
    return courses.filter((course) =>
      course.name.toLocaleLowerCase().includes(query),
    );
  });
  let selectedCourse = $derived.by(() => courses.find((c) => c.key === selectedCourseKey) ?? null);

  // ─── Act-based chapter structure ──────────────────────────────────────
  /// The chapter's acts (幕). Primary play structure.
  let acts = $derived.by(() => currentCase?.acts ?? []);
  let currentAct = $derived.by(() => acts[actIndex] ?? null);
  /// The full Court Record pool (every card the chapter can ever surface).
  let chapterEvidence = $derived.by(() => currentCase?.evidence ?? []);
  /// Cards revealed in the CURRENT investigation act (the read-this carousel).
  let actEvidence = $derived.by(() => {
    if (!currentAct || currentAct.kind !== "investigation") return [];
    return currentAct.evidenceIds
      .map((id) => chapterEvidence.find((e) => e.id === id))
      .filter((e): e is DetectiveEvidence => !!e);
  });
  /// Cards discovered so far — every investigation act up to and including the
  /// current one. This is what the Court Record overlay offers during a press.
  let recordEvidence = $derived.by(() => {
    const ids = new Set<string>();
    for (let i = 0; i <= actIndex && i < acts.length; i++) {
      const a = acts[i];
      if (a.kind === "investigation") a.evidenceIds.forEach((id) => ids.add(id));
    }
    return chapterEvidence.filter((e) => ids.has(e.id));
  });
  /// Statements of the current testimony act.
  let actTestimony = $derived.by(() =>
    currentAct?.kind === "testimony" ? currentAct.testimony : [],
  );
  /// Lies in the current act (one per testimony act).
  let actLies = $derived.by(() => actTestimony.filter((t) => t.isFalse && t.keyEvidenceId));
  /// Every lie across the whole chapter — for progress + memory accounting.
  let allLies = $derived.by(() =>
    acts
      .flatMap((a) => (a.kind === "testimony" ? a.testimony : []))
      .filter((t) => t.isFalse && t.keyEvidenceId),
  );
  let totalLies = $derived.by(() => allLies.length);
  let witnessName = $derived.by(
    () => currentAct?.witnessName?.trim() || currentCase?.witnessName?.trim() || "証人",
  );
  let witnessRole = $derived.by(
    () => currentAct?.witnessRole?.trim() || currentCase?.witnessRole?.trim() || "",
  );
  let scenarioText = $derived.by(() => currentCase?.scenario?.trim() || "");
  let currentEvidence = $derived.by(() => actEvidence[evidenceIndex] ?? null);
  let currentXe = $derived.by(() => actTestimony[xeIndex] ?? null);
  let lieStatement = $derived.by(() => actTestimony.find((t) => t.isFalse) ?? null);
  /// Proof card backing the current act's lie (shown on chapter clear).
  let finalProof = $derived.by(() =>
    lieStatement ? chapterEvidence.find((e) => e.id === lieStatement.keyEvidenceId) ?? null : null,
  );
  let presentedEvidence = $derived.by(() => recordEvidence.find((e) => e.id === presentedEvidenceId) ?? null);
  let questionText = $derived.by(() => currentCase?.finalQuestion?.trim() || "この事件、真相はどこにある？");
  /// True when the current act is the last one in the chapter.
  let isLastAct = $derived.by(() => acts.length > 0 && actIndex >= acts.length - 1);
  /// Knowledge points that this chapter actually covered (post-archive view).
  let coveredPoints = $derived.by(() => {
    if (!currentCase) return [] as KnowledgePoint[];
    const coveredIds = new Set((currentCase.coverage ?? []).map((c) => c.pointId));
    return (currentCase.knowledgePoints ?? []).filter((p) => coveredIds.has(p.id));
  });

  /// Whether to show the cross-exam Court Record overlay.
  let recordOpen = $derived.by(() => scene === "cross_exam" && xeMode === "presenting");

  /// Protagonist detective's pose — driven by game STATE only (never by which
  /// statement is the lie, to avoid spoiling it). Keyed to illustration slots.
  ///   normal    — listening to testimony
  ///   think     — pressing a statement (詳しく聞く)
  ///   point     — presenting evidence / objection
  ///   confident — case solved (異議成立)
  ///   sweat     — wrong / cornered
  type DetectivePose = "normal" | "think" | "point" | "confident" | "sweat";
  let detectivePose = $derived.by<DetectivePose>(() => {
    if (scene === "result_pass") return "confident";
    if (scene === "result_fail" || scene === "unresolved") return "sweat";
    if (scene === "cross_exam") {
      if (xeMode === "presenting") return "point";
      if (xeMode === "pressed") return "think";
    }
    return "normal";
  });

  let themeUnlisten: (() => void) | null = null;
  let appThemeUnlisten: (() => void) | null = null;

  onMount(async () => {
    // Runs as a standalone webview in the Copilot window, so it owns its theme.
    document.documentElement.setAttribute("data-aux-surface", "detective");
    document.body.setAttribute("data-aux-surface", "detective");
    await syncAuxiliaryTheme();
    themeUnlisten = await listen<string>("theme-changed", (event) => applyAuxiliaryTheme(event.payload)).catch(() => null);
    appThemeUnlisten = await listen("app-theme-changed", () => void syncAuxiliaryTheme()).catch(() => null);
    void refreshContext();
  });

  onDestroy(() => {
    themeUnlisten?.();
    appThemeUnlisten?.();
    document.documentElement.removeAttribute("data-aux-surface");
    document.body.removeAttribute("data-aux-surface");
  });

  // Advance the loading label through the three generation passes while a
  // chapter is being built (the backend runs them sequentially, no events).
  $effect(() => {
    if (scene !== "loading") {
      loadingStage = 0;
      return;
    }
    loadingStage = 0;
    const id = setInterval(() => {
      loadingStage = Math.min(loadingStage + 1, LOADING_STAGES.length - 1);
    }, 9000);
    return () => clearInterval(id);
  });

  async function refreshContext() {
    loading = true;
    error = "";
    try {
      context = await invoke<DetectiveContext>("detective_get_context");
      includedKeys = new Set(context.includedCourseKeys ?? []);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function toggleIncluded(courseKey: string) {
    const next = new Set(includedKeys);
    if (next.has(courseKey)) next.delete(courseKey);
    else next.add(courseKey);
    await saveIncluded(next);
  }

  async function toggleAll() {
    if (allSelected) {
      await saveIncluded(new Set());
    } else {
      await saveIncluded(new Set(selectableCourses.map((c) => c.key)));
    }
  }

  async function saveIncluded(next: Set<string>) {
    includedKeys = next;
    try {
      await invoke("detective_save_included_courses", { included: [...next] });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function finishSelection() {
    editingSelection = false;
    courseQuery = "";
  }

  function openSelection() {
    editingSelection = true;
    courseQuery = "";
  }

  /// Focus a course's campaign and show its chapter list.
  async function openChapters(courseKey: string) {
    campaignCourseKey = courseKey;
    selectedCourseKey = courseKey;
    scene = "chapters";
    await loadChapters();
  }

  async function loadChapters() {
    if (!campaignCourseKey) return;
    chaptersLoading = true;
    error = "";
    try {
      chapters = await invoke<DetectiveChapterInfo[]>("detective_get_chapters", {
        courseKey: campaignCourseKey,
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      chapters = [];
    } finally {
      chaptersLoading = false;
    }
  }

  function backToTitle() {
    scene = "list";
    chapters = [];
  }

  /// Rebuild the focused course's campaign world from scratch (carries over
  /// progress). Useful to backfill the meta-arc / finale onto older campaigns.
  async function regenerateCampaign() {
    if (!campaignCourseKey || regenerating) return;
    regenerating = true;
    error = "";
    try {
      await invoke<DetectiveCampaign>("detective_generate_campaign", {
        courseKey: campaignCourseKey,
        force: true,
      });
      await refreshContext();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      regenerating = false;
    }
  }

  /// Generate (or load the cached) chapter case for one Live note, then play it.
  async function playChapter(liveId: string, force = false) {
    if (!campaignCourseKey) return;
    selectedCourseKey = campaignCourseKey;
    currentLiveId = liveId;
    revealedAtStart = (activeCampaign?.metaArc ?? []).filter((r) => r.unlocked).length;
    progressAtStart = activeCampaign?.metaProgress ?? 0;
    newRevelations = [];
    campaignJustCompleted = false;
    actIndex = 0;
    evidenceIndex = 0;
    xeIndex = 0;
    xeMode = "idle";
    presentedEvidenceId = "";
    bustedLieIds = new Set();
    lives = MAX_LIVES;
    currentCase = null;
    resolvedOk = false;
    scene = "loading";
    error = "";
    try {
      currentCase = await invoke<DetectiveCase>("detective_generate_chapter", {
        courseKey: campaignCourseKey,
        liveId,
        force,
      });
    } catch (e) {
      currentCase = null;
      error = e instanceof Error ? e.message : String(e);
      scene = "ai_error";
      return;
    }
    startChapter();
  }

  /// Open the chapter at its first act.
  function startChapter() {
    actIndex = 0;
    if (acts.length === 0) {
      // No acts at all — nothing to play; archive as a passive review.
      void closeCaseAndArchive(3, false);
      return;
    }
    enterActIntro();
  }

  /// Show the current act's story card.
  function enterActIntro() {
    evidenceIndex = 0;
    xeIndex = 0;
    xeMode = "idle";
    presentedEvidenceId = "";
    scene = "act_intro";
  }

  /// Leave the act's story card and enter its body (read evidence or testify).
  function enterActBody() {
    if (scene !== "act_intro" || !currentAct) return;
    if (currentAct.kind === "testimony" && currentAct.testimony.some((t) => t.isFalse)) {
      xeIndex = 0;
      xeMode = "idle";
      presentedEvidenceId = "";
      scene = "cross_exam";
    } else {
      // investigation act (or a degenerate testimony act with no lie).
      evidenceIndex = 0;
      scene = "investigate";
    }
  }

  /// Advance to the next act, or finish the chapter if this was the last one.
  function advanceAct() {
    if (actIndex >= acts.length - 1) {
      resolvedOk = true;
      scene = "result_pass";
      return;
    }
    actIndex += 1;
    enterActIntro();
  }

  async function retryCase() {
    if (!campaignCourseKey || !currentLiveId) {
      scene = "chapters";
      return;
    }
    await playChapter(currentLiveId);
  }

  /// Return to the focused campaign's chapter list, refreshing play status.
  async function backToChapters() {
    scene = "chapters";
    xeMode = "idle";
    presentedEvidenceId = "";
    xeIndex = 0;
    evidenceIndex = 0;
    actIndex = 0;
    currentCase = null;
    await loadChapters();
  }

  function abortCase() {
    scene = "chapters";
    xeMode = "idle";
    presentedEvidenceId = "";
    xeIndex = 0;
    evidenceIndex = 0;
    actIndex = 0;
  }

  // ─── Investigate phase ────────────────────────────────────────────────

  function prevEvidence() {
    if (scene !== "investigate") return;
    const n = Math.max(actEvidence.length, 1);
    evidenceIndex = (evidenceIndex - 1 + n) % n;
  }
  function nextEvidence() {
    if (scene !== "investigate") return;
    evidenceIndex = (evidenceIndex + 1) % Math.max(actEvidence.length, 1);
  }

  // ─── Cross-examination interactions ───────────────────────────────────

  function nextStatement() {
    if (scene !== "cross_exam" || xeMode === "presenting") return;
    xeMode = "idle";
    xeIndex = (xeIndex + 1) % Math.max(actTestimony.length, 1);
  }
  function prevStatement() {
    if (scene !== "cross_exam" || xeMode === "presenting") return;
    xeMode = "idle";
    const n = Math.max(actTestimony.length, 1);
    xeIndex = (xeIndex - 1 + n) % n;
  }
  function pressStatement() {
    if (scene !== "cross_exam") return;
    xeMode = "pressed";
  }
  /// Click on the textbox during cross-exam:
  /// - idle: advance to the next testimony statement (looping).
  /// - pressed: close the press response and return to testimony.
  /// - presenting: noop (record overlay is in control).
  function advanceTextbox() {
    if (scene !== "cross_exam") return;
    if (xeMode === "presenting") return;
    if (xeMode === "pressed") {
      xeMode = "idle";
      return;
    }
    if (actTestimony.length > 1) {
      xeIndex = (xeIndex + 1) % actTestimony.length;
    }
  }
  function closePress() {
    if (scene !== "cross_exam" || xeMode !== "pressed") return;
    xeMode = "idle";
  }
  function openPresent() {
    if (scene !== "cross_exam") return;
    presentedEvidenceId = "";
    xeMode = "presenting";
  }
  function cancelPresent() {
    if (scene !== "cross_exam" || xeMode !== "presenting") return;
    presentedEvidenceId = "";
    xeMode = "idle";
  }
  function pickEvidenceForPress(id: string) {
    if (scene !== "cross_exam" || xeMode !== "presenting") return;
    presentedEvidenceId = id;
  }

  function submitPresent() {
    if (scene !== "cross_exam" || xeMode !== "presenting") return;
    if (!presentedEvidenceId) return;
    // The current statement matches if (a) it's a lie AND (b) the presented
    // evidence id matches its key. We check the CURRENT statement specifically
    // (not "any lie") so the player has to pin the contradiction to the right
    // line.
    const currentIsLie = currentXe?.isFalse === true;
    const evidenceMatches =
      currentIsLie && currentXe?.keyEvidenceId === presentedEvidenceId;

    if (evidenceMatches && currentXe) {
      // Bust this lie. When the current act's lies are all down, the act clears.
      const nextBusted = new Set(bustedLieIds);
      nextBusted.add(currentXe.id);
      bustedLieIds = nextBusted;
      xeMode = "idle";
      presentedEvidenceId = "";

      const stillStanding = actLies.filter((lie) => !nextBusted.has(lie.id));
      if (stillStanding.length === 0) {
        // This act's contradiction resolved.
        if (isLastAct) {
          resolvedOk = true;
          scene = "result_pass";
        } else {
          scene = "act_clear";
          setTimeout(() => {
            if (scene === "act_clear") advanceAct();
          }, 1500);
        }
        return;
      }
      // Rare: an act with more than one lie — nudge to the next statement.
      xeIndex = (xeIndex + 1) % Math.max(actTestimony.length, 1);
      return;
    }

    // Wrong present — lose a life, brief overlay, return to cross-exam.
    lives = Math.max(0, lives - 1);
    shakeKey += 1;
    xeMode = "idle";
    presentedEvidenceId = "";
    if (lives <= 0) {
      scene = "result_fail";
      setTimeout(() => {
        if (scene === "result_fail") scene = "unresolved";
      }, 1500);
      return;
    }
    scene = "result_fail";
    setTimeout(() => {
      if (scene === "result_fail") scene = "cross_exam";
    }, 1500);
  }

  function advance() {
    switch (scene) {
      case "result_pass":
        // Win: confidence auto-derived from lives remaining (3 → 5, 2 → 4, 1 → 3).
        void closeCaseAndArchive((lives >= 3 ? 5 : lives >= 2 ? 4 : 3) as 3 | 4 | 5, false);
        return;
      case "unresolved":
        void closeCaseAndArchive(1, true);
        return;
      case "closed":
        void backToChapters();
        return;
    }
  }

  async function closeCaseAndArchive(value: 1 | 3 | 4 | 5, asUnresolved: boolean) {
    if (!selectedCourse || !currentCase) return;
    scene = "closing";
    error = "";
    try {
      // Partition lies into busted vs missed for memory accounting.
      const bustedLies = allLies.filter((lie) => bustedLieIds.has(lie.id));
      const missedLies = allLies.filter((lie) => !bustedLieIds.has(lie.id));

      // Headline text uses the first busted lie (win) or first missed (loss).
      const headlineLie = bustedLies[0] ?? missedLies[0] ?? null;

      const relation = asUnresolved
        ? `${missedLies.length}件の嘘を見抜けず、迷宮入り。`
        : bustedLies.length > 0
          ? `${bustedLies.length}件の嘘を突きつけた。${headlineLie ? `代表：「${truncate(headlineLie.text, 50)}」` : ""}`
          : `事件を確信度 ${value} で閉廷。`;
      const deductionLines: string[] = [];
      for (const lie of bustedLies) {
        const proof = chapterEvidence.find((e) => e.id === lie.keyEvidenceId);
        deductionLines.push(
          `◆ 暴いた嘘：${lie.text}${proof ? `\n  → ${proof.title}：${truncate(proof.excerpt, 80)}` : ""}`,
        );
      }
      for (const lie of missedLies) {
        deductionLines.push(`◆ 見逃した嘘：${lie.text}`);
      }
      const deduction = `${questionText}\n\n${deductionLines.join("\n\n") || "（証言なし）"}`;
      const now = Date.now();
      const selectedIds: string[] = [];
      for (const lie of bustedLies) {
        selectedIds.push(`testimony:${lie.id}`);
        if (lie.keyEvidenceId) selectedIds.push(lie.keyEvidenceId);
      }

      const result: DetectiveCaseResult = {
        id: `${currentCase.id}:${now}`,
        caseId: currentCase.id,
        courseKey: currentCase.courseKey,
        courseName: currentCase.courseName || selectedCourse.name,
        caseTitle: currentCase.title,
        caseType: currentCase.caseType,
        selectedEvidenceIds: selectedIds,
        relation,
        deduction,
        closedAt: now,
        confidence: value,
      };
      await invoke<DetectiveCaseResult[]>("detective_save_case_result", { result });

      // Cross-session memory: tell the backend what was busted vs missed.
      // Topic = first highlight or a truncated statement.
      const topicOf = (lie: DetectiveTestimony) =>
        (lie.highlights ?? [])[0]?.trim() || truncate(lie.text, 40);
      const bustedTopics = bustedLies.map(topicOf).filter(Boolean);
      const missedTopics = missedLies.map(topicOf).filter(Boolean);
      const evidenceTitles = chapterEvidence.map((e) => e.title).filter(Boolean);
      try {
        await invoke("detective_save_memory_outcome", {
          bustedTopics,
          missedTopics,
          courseName: currentCase.courseName || selectedCourse.name,
          evidenceTitles,
        });
      } catch (memErr) {
        // Memory failures are non-fatal — log and continue.
        console.warn("[Detective] memory update failed", memErr);
      }

      if (asUnresolved && missedLies.length > 0) {
        const allDoubts = context.courses.flatMap((c) => c.doubts);
        const fresh: DetectiveDoubt[] = missedLies.map((lie, i) => ({
          id: `${now}-${i}-${Math.random().toString(36).slice(2, 8)}`,
          courseName: selectedCourse.name,
          note: `見抜けず：${truncate(lie.text, 80)}`,
          evidenceIds: lie.keyEvidenceId ? [lie.keyEvidenceId] : [],
          createdAt: now,
          dueAt: now + 24 * 60 * 60 * 1000,
        }));
        await invoke("detective_save_doubts", { doubts: [...fresh, ...allDoubts] });
      }

      lastClosedTitle = currentCase.title || selectedCourse.name;
      await refreshContext();
      // Surface any meta-arc truths unlocked by clearing this chapter.
      const unlockedNow = (activeCampaign?.metaArc ?? []).filter((r) => r.unlocked);
      newRevelations = unlockedNow.slice(revealedAtStart);
      campaignJustCompleted =
        !!activeCampaign &&
        activeCampaign.metaProgress >= 100 &&
        progressAtStart < 100 &&
        !!activeCampaign.finale.trim();
      // On completion, rewrite the finale to pay off the real accumulated canon
      // (best-effort; keeps the static finale on failure).
      if (campaignJustCompleted && selectedCourse) {
        try {
          await invoke("detective_finalize_finale", { courseKey: selectedCourse.key });
          await refreshContext();
        } catch (finaleErr) {
          console.warn("[Detective] finale finalize failed", finaleErr);
        }
      }
      scene = "closed";
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      scene = asUnresolved ? "unresolved" : "result_pass";
    }
  }

  /// Split a testimony's text into runs, marking which runs are highlights.
  /// Returns [{ text, mark }] used to render <mark> spans.
  interface HiSeg { text: string; mark: boolean }
  function highlightSegments(text: string, highlights: string[]): HiSeg[] {
    if (!text) return [];
    const valid = (highlights ?? [])
      .map((h) => h.trim())
      .filter((h) => h.length > 0 && text.includes(h))
      // Longest first so we don't half-match.
      .sort((a, b) => b.length - a.length);
    if (valid.length === 0) return [{ text, mark: false }];
    const segs: HiSeg[] = [];
    let i = 0;
    while (i < text.length) {
      let matched: string | null = null;
      for (const h of valid) {
        if (text.startsWith(h, i)) { matched = h; break; }
      }
      if (matched) {
        segs.push({ text: matched, mark: true });
        i += matched.length;
      } else {
        let j = i + 1;
        while (j < text.length) {
          let ahead = false;
          for (const h of valid) {
            if (text.startsWith(h, j)) { ahead = true; break; }
          }
          if (ahead) break;
          j += 1;
        }
        segs.push({ text: text.slice(i, j), mark: false });
        i = j;
      }
    }
    return segs;
  }

  function sourceLabel(type: DetectiveEvidence["sourceType"]): string {
    if (type === "live") return "ライブ";
    if (type === "doubt") return "疑念";
    return "通知";
  }

  function truncate(text: string, max: number): string {
    if (!text) return "";
    const clean = text.replace(/\s+/g, " ").trim();
    return clean.length > max ? clean.slice(0, max) + "…" : clean;
  }

</script>

<div class="detective" class:trial={scene !== "list"}>
  {#if scene === "list"}
    <section class="case-pick">
      {#if error}<div class="banner error">{error}</div>{/if}

      {#if loading && courses.length === 0}
        <div class="banner">科目一覧を読み込んでいる……</div>
      {:else if courses.length === 0}
        <div class="empty-card">
          <h3>科目データが見つかりません</h3>
          <p>ライブメモか試験関連の通知を取得すると、ここに科目が並びます。</p>
          <button class="ghost-btn" onclick={refreshContext}>再読み込み</button>
        </div>
      {:else}
        <div class="title-screen">
          <div class="ts-hero">
            <span class="ts-emblem" aria-hidden="true"><img class="ts-logo" src="/naruhodo.png" alt="" /></span>
            <h1 class="ts-title">なるほど</h1>
            <p class="ts-tagline">講義に潜む矛盾を、暴け。</p>
          </div>

          {#if hasSelection && activeCourses.length > 1}
            <div class="ts-campaign-switch" aria-label="キャンペーンを選ぶ">
              {#each activeCourses as c}
                <button
                  class="campaign-chip"
                  class:active={campaignCourse?.key === c.key}
                  onclick={() => (campaignCourseKey = c.key)}
                >{c.name}</button>
              {/each}
            </div>
          {/if}

          {#if hasSelection && activeCampaign}
            <div class="ts-world">
              <span class="ts-world-label">{activeCampaign.worldLabel}</span>
              {#if activeCampaign.tagline}
                <p class="ts-world-tagline">{activeCampaign.tagline}</p>
              {/if}
              <div class="ts-world-meter" aria-label="物語の進行度">
                <span
                  class="ts-world-fill"
                  style="width: {Math.min(100, Math.max(0, activeCampaign.metaProgress))}%"
                ></span>
              </div>
              <span class="ts-world-meta">
                真相 {Math.min(100, Math.max(0, activeCampaign.metaProgress))}%
                {#if activeCampaign.chapters.length > 0} ・ 第{activeCampaign.chapters.length}章まで{/if}
              </span>
            </div>
          {/if}

          {#if hasSelection}
            <button class="ts-start" onclick={() => campaignCourse && openChapters(campaignCourse.key)}>
              <span class="ts-start-row">
                <span class="ts-start-main">章を選ぶ</span>
                <span class="ts-start-play" aria-hidden="true">▶</span>
              </span>
              <span class="ts-start-sub">
                講義ごとの事件に挑む{#if stat_pending > 0} ・ 前回の疑念 {stat_pending}件が再登場{/if}
              </span>
            </button>
          {:else}
            <button class="ts-start" onclick={openSelection}>
              <span class="ts-start-row">
                <span class="ts-start-main">科目を選ぶ</span>
                <span class="ts-start-play" aria-hidden="true">▶</span>
              </span>
              <span class="ts-start-sub">調べたい科目をえらぼう</span>
            </button>
          {/if}
        </div>
      {/if}
    </section>

    {#if hasSelection && courses.length > 0}
      <div class="title-corner" role="group" aria-label="統計と編集">
        <span class="tc-stats" aria-label="累積">
          <span><b>{stat_resolved}</b> 解決</span>
          <span class="tc-sep">·</span>
          <span><b>{stat_busted}</b> 暴いた嘘</span>
        </span>
        <span class="tc-div" aria-hidden="true"></span>
        <button class="tc-edit" onclick={openSelection}>対象を編集</button>
      </div>
    {/if}

    {#if editingSelection}
      <div
        class="modal-backdrop"
        role="button"
        tabindex="-1"
        onclick={(e) => { if (e.target === e.currentTarget) finishSelection(); }}
        onkeydown={(e) => { if (e.key === "Escape") finishSelection(); }}
      >
        <div class="modal" role="dialog" aria-label="科目を選ぶ" aria-modal="true">
          <header class="modal-head">
            <h2>科目を選ぶ</h2>
            <button class="ghost-btn small" onclick={toggleAll} disabled={selectableCourses.length === 0}>
              {allSelected ? "すべて解除" : "すべて選択"}
            </button>
          </header>

          {#if courses.length > 8}
            <input
              class="course-search"
              bind:value={courseQuery}
              placeholder="科目名で絞り込み"
              aria-label="科目名で絞り込み"
            />
          {/if}

          {#if visibleCourses.length === 0}
            <div class="empty-card">
              <h3>該当なし</h3>
              <p>検索語を変えてください。</p>
            </div>
          {:else}
            <ul class="select-list">
              {#each visibleCourses as course (course.key)}
                {@const isIncluded = includedKeys.has(course.key)}
                {@const playable = courseHasMaterial(course)}
                <li>
                  <button
                    class="select-row"
                    class:on={isIncluded}
                    disabled={!playable}
                    onclick={() => toggleIncluded(course.key)}
                  >
                    <span class="sr-name">{course.name}</span>
                    {#if !playable}
                      <span class="sr-empty">素材なし</span>
                    {:else if isIncluded}
                      <span class="sr-check" aria-hidden="true">✓</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}

          <footer class="modal-foot">
            <button class="primary-btn wide" onclick={finishSelection}>
              完了（{activeCourses.length}）
            </button>
          </footer>
        </div>
      </div>
    {/if}
  {:else if scene === "chapters"}
    <section class="chapter-screen">
      <header class="ch-head">
        <button class="ghost-btn small" onclick={backToTitle} aria-label="戻る">❮ 戻る</button>
        <div class="ch-head-title">
          <h1>{campaignCourse?.name ?? "キャンペーン"}</h1>
          {#if activeCampaign}<span class="ch-world">{activeCampaign.worldLabel}</span>{/if}
        </div>
        <button
          class="ghost-btn small"
          onclick={regenerateCampaign}
          disabled={regenerating || !campaignCourseKey}
          title="世界観を作り直す（進行度は保持）"
        >{regenerating ? "生成中…" : "世界観を作り直す"}</button>
      </header>

      {#if error}<div class="banner error">{error}</div>{/if}

      {#if chaptersLoading}
        <div class="banner">章を読み込んでいる……</div>
      {:else if chapters.length === 0}
        <div class="empty-card">
          <h3>この科目にはまだライブメモがありません</h3>
          <p>ライブを記録すると、その回が 1 つの章になります。</p>
        </div>
      {:else}
        <ul class="chapter-list">
          {#each chapters as ch}
            <li>
              <button
                class="chapter-row"
                class:played={ch.played}
                class:locked={ch.locked}
                disabled={ch.locked}
                onclick={() => !ch.locked && playChapter(ch.liveId)}
              >
                <span class="ch-no">{ch.aligned ? `第${ch.index}回` : "未整理"}</span>
                <span class="ch-main">
                  <strong>{ch.title}</strong>
                  <span class="ch-status">
                    {#if ch.locked}
                      未配信 ・ 講義が記録されると解放
                    {:else if !ch.aligned}
                      {ch.generated ? "回数未確定（生成済み）" : "回数未確定 ・ 解いて整理"}
                    {:else if ch.played}
                      クリア済み{#if ch.bestConfidence > 0} ・ 確信度 {ch.bestConfidence}/5{/if}
                    {:else if ch.generated}
                      未解決（生成済み）
                    {:else}
                      未着手
                    {/if}
                  </span>
                </span>
                <span class="ch-go" aria-hidden="true">
                  {#if ch.locked}🔒{:else if ch.played}再挑戦 ▶{:else}▶{/if}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      {#if activeCampaign && activeCampaign.metaArc.length > 0}
        <section class="truth-file">
          <header class="tf-head">
            <span class="tf-eyebrow">真相ファイル · 暗線</span>
            <span class="tf-progress">真相 {Math.min(100, Math.max(0, activeCampaign.metaProgress))}%</span>
          </header>
          <ol class="tf-list">
            {#each activeCampaign.metaArc as rev}
              <li class="tf-item" class:locked={!rev.unlocked}>
                {#if rev.unlocked}
                  <span class="tf-stage">{rev.stage}</span>
                  <div class="tf-body">
                    <strong>{rev.title}</strong>
                    <p>{rev.reveal}</p>
                  </div>
                {:else}
                  <span class="tf-stage locked" aria-hidden="true">？</span>
                  <div class="tf-body">
                    <strong>？？？</strong>
                    <p class="tf-lock">真相 {rev.threshold}% で解放</p>
                  </div>
                {/if}
              </li>
            {/each}
          </ol>
        </section>
      {/if}

      {#if activeCampaign && activeCampaign.metaProgress >= 100 && activeCampaign.finale.trim()}
        <section class="finale-panel">
          <span class="finale-stamp small">完 結</span>
          <p class="finale-text">{activeCampaign.finale}</p>
        </section>
      {/if}
    </section>
  {:else if selectedCourse}
    <div class="stage">
      {#key shakeKey}
        <div class="stage-frame shake-{shakeKey % 2}">
          <!-- Floating corner controls: top-left record button (cross_exam only), top-right lives + close -->
          {#if scene === "cross_exam"}
            <button
              class="corner-btn record"
              onclick={openPresent}
              aria-label="証拠記録を開く"
              title="証拠記録"
            >
              <i aria-hidden="true">≡</i>
              <span>証拠記録</span>
            </button>
          {/if}
          <div class="corner-right">
            {#if (scene === "act_intro" || scene === "investigate" || scene === "cross_exam") && acts.length > 0}
              <span class="act-counter" aria-label="幕の進捗">第{actIndex + 1}幕 / {acts.length}</span>
            {/if}
            {#if (scene === "investigate" || scene === "cross_exam") && currentAct?.seedsMeta}
              <span class="meta-hint" title="この幕には暗線につながる手がかりがある" aria-label="気になる手がかり">◆ 気になる手がかり</span>
            {/if}
            {#if scene === "cross_exam"}
              <span class="lives" aria-label="残ライフ">
                {#each Array.from({ length: MAX_LIVES }) as _, i}
                  <i class:lost={i >= lives}>♥</i>
                {/each}
              </span>
              {#if totalLies > 0}
                <span class="lie-counter" aria-label="矛盾の進捗">
                  矛盾 {bustedLieIds.size} / {totalLies}
                </span>
              {/if}
            {/if}
            <button class="corner-btn close" onclick={abortCase} aria-label="退廷">×</button>
          </div>

          {#if scene === "loading"}
            <div class="stage-center">
              <div class="loading-card">
                <span class="loading-spinner" aria-hidden="true"></span>
                <p>{LOADING_STAGES[loadingStage]}</p>
                <span class="loading-sub">ライブメモから事件を構成しています</span>
              </div>
            </div>
          {:else if scene === "ai_error"}
            <div class="stage-center">
              <div class="error-card">
                <p class="error-msg">{error || "AI 生成に失敗しました。"}</p>
                <p class="error-sub">設定で AI を有効化してから、もう一度試そう。</p>
                <div class="error-actions">
                  <button class="primary-btn" onclick={retryCase}>もう一度試す ▶</button>
                  <button class="ghost-btn" onclick={backToChapters}>章一覧に戻る</button>
                </div>
              </div>
            </div>

          {:else if scene === "act_intro" && currentAct}
            <div class="phase phase-act-intro">
              <section class="act-card">
                {#if actIndex === 0 && currentCase?.briefing?.trim()}
                  <div class="case-file">
                    <span class="case-file-tag">事件ファイル</span>
                    <p>{currentCase.briefing}</p>
                  </div>
                {/if}
                {#if actIndex === 0 && scenarioText}
                  <p class="act-prologue">{scenarioText}</p>
                {/if}
                <span class="act-eyebrow">
                  第{actIndex + 1}幕
                  · {currentAct.kind === "investigation" ? "捜査" : "尋問"}
                  {#if currentAct.seedsMeta}<span class="act-seed" title="伏線">◆</span>{/if}
                </span>
                <h1 class="act-title">{currentAct.title?.trim() || `第${actIndex + 1}幕`}</h1>
                {#if currentAct.location}
                  <span class="act-location">{currentAct.location}</span>
                {/if}
                {#if currentAct.narrative}
                  <p class="act-narrative">{currentAct.narrative}</p>
                {/if}
              </section>
              <footer class="phase-footer">
                <button class="primary-btn wide" onclick={enterActBody}>
                  {currentAct.kind === "investigation" ? "証拠を調べる ▶" : "尋問を始める ▶"}
                </button>
              </footer>
            </div>

          {:else if scene === "investigate"}
            <div class="phase phase-investigate">
              <section class="scenario-block">
                <span class="scenario-eyebrow">捜査 · 第{actIndex + 1}幕</span>
                <h1 class="scenario-title">{currentAct?.title?.trim() || currentCase?.title?.trim() || selectedCourse.name}</h1>
                {#if currentAct?.narrative}
                  <p class="scenario-body">{currentAct.narrative}</p>
                {/if}
              </section>

              {#if actEvidence.length > 0 && currentEvidence}
                <section class="evidence-block">
                  <article class="evidence-viewer">
                    <div class="ev-top">
                      <span class={`src-pill src-${currentEvidence.sourceType}`}>{sourceLabel(currentEvidence.sourceType)}</span>
                      <span class="evidence-dots" aria-hidden="true">
                        {#each actEvidence as _, i}
                          <i class:on={i === evidenceIndex}></i>
                        {/each}
                      </span>
                    </div>
                    <h2 class="ev-title">{currentEvidence.title}</h2>
                    <p class="ev-body">{currentEvidence.excerpt}</p>
                  </article>
                  <div class="evidence-nav">
                    <button class="ghost-btn" onclick={prevEvidence} disabled={actEvidence.length <= 1}>
                      ❮ 前の証拠
                    </button>
                    <button class="ghost-btn" onclick={nextEvidence} disabled={actEvidence.length <= 1}>
                      次の証拠 ❯
                    </button>
                  </div>
                </section>
              {/if}

              <footer class="phase-footer">
                <button class="primary-btn wide" onclick={advanceAct}>
                  {isLastAct ? "事件を綴じる ▶" : "次へ ▶"}
                </button>
              </footer>
            </div>

          {:else if scene === "cross_exam" && currentXe}
            <!-- Trial panel: witness portrait + textbox + actions all anchored together -->
            <div class="phase phase-cross-exam">
              <article class="trial-panel" class:pressed={xeMode === "pressed"}>
                <div class="cast-row">
                  <!-- Protagonist detective — illustration slot (art TBD), pose by game state -->
                  <figure class="cast detective" data-character="detective" data-pose={detectivePose}>
                    <span class="cast-art" aria-hidden="true">探</span>
                    <figcaption>
                      <strong>探偵</strong>
                      <span class="cast-pose">{detectivePose}</span>
                    </figcaption>
                  </figure>
                  <!-- Witness -->
                  <figure class="cast witness" data-character="witness">
                    <span class="cast-art" aria-hidden="true">{witnessName.charAt(0) || "証"}</span>
                    <figcaption>
                      <strong>{witnessName}</strong>
                      {#if witnessRole}<span>{witnessRole}</span>{/if}
                    </figcaption>
                  </figure>
                </div>

                <button
                  class="textbox"
                  class:pressed={xeMode === "pressed"}
                  onclick={advanceTextbox}
                  type="button"
                >
                  <span class="textbox-tag">
                    {#if xeMode === "pressed"}{witnessName} の反応{:else}証言 {xeIndex + 1} / {actTestimony.length}{/if}
                  </span>
                  {#if xeMode === "pressed"}
                    <p class="textbox-text reply">
                      {currentXe.pressResponse?.trim() || "……（証人は黙ったまま、答えなかった。）"}
                    </p>
                  {:else}
                    <p class="textbox-text testimony">
                      「{#each highlightSegments(currentXe.text, currentXe.highlights ?? []) as seg}{#if seg.mark}<mark class="hi">{seg.text}</mark>{:else}{seg.text}{/if}{/each}」
                    </p>
                  {/if}
                  <span class="textbox-advance">▼</span>
                </button>

                <footer class="trial-actions">
                  <button
                    class="nav-arrow"
                    onclick={prevStatement}
                    disabled={xeMode !== "idle" || actTestimony.length <= 1}
                    aria-label="前の証言"
                  >❮</button>
                  <button
                    class="action-btn press"
                    onclick={pressStatement}
                    disabled={xeMode === "pressed"}
                  >
                    <strong>ゆさぶる</strong>
                    <span>詳しく聞く</span>
                  </button>
                  <button
                    class="action-btn present"
                    onclick={openPresent}
                    disabled={xeMode === "presenting"}
                  >
                    <strong>突きつける</strong>
                    <span>証拠で反論</span>
                  </button>
                  <button
                    class="nav-arrow"
                    onclick={nextStatement}
                    disabled={xeMode !== "idle" || actTestimony.length <= 1}
                    aria-label="次の証言"
                  >❯</button>
                </footer>
              </article>
            </div>

          {:else if scene === "act_clear"}
            <div class="act-clear-overlay" aria-live="polite">
              <span class="cutin-text">「異議あり！」</span>
              <p>第{actIndex + 1}幕、突破。</p>
            </div>

          {:else if scene === "result_pass"}
            <div class="stage-center result-stack">
              <div class="objection-cutin" aria-hidden="true">
                <span class="cutin-text">「異議あり！」</span>
              </div>
              <div class="result-pass">
                <span class="rp-stamp">真相解明</span>
                {#if lieStatement}
                  <p class="rp-lie">嘘の証言：「{lieStatement.text}」</p>
                {/if}
                {#if finalProof}
                  <div class="rp-evidence">
                    <span class={`src-pill src-${finalProof.sourceType}`}>{sourceLabel(finalProof.sourceType)}</span>
                    <strong>{finalProof.title}</strong>
                    <p>{finalProof.excerpt}</p>
                  </div>
                {/if}
                <p class="rp-progress">矛盾 {bustedLieIds.size} / {totalLies} を突きつけた。</p>
                <button class="primary-btn" onclick={advance}>事件を綴じる ▶</button>
              </div>
            </div>
          {:else if scene === "result_fail"}
            <div class="fail-overlay" aria-live="polite">
              <span class="dameda">ダメだ！</span>
              <p>残ライフ {lives} / {MAX_LIVES}</p>
            </div>
          {:else if scene === "unresolved"}
            <div class="stage-center">
              <div class="unresolved-card">
                <span class="unresolved-stamp">迷宮入り</span>
                <p>嘘を暴けなかった。疑念として記録に残す。</p>
                <button class="primary-btn" onclick={advance}>事件を綴じる ▶</button>
              </div>
            </div>
          {:else if scene === "closing"}
            <div class="stage-center">
              <div class="closing-card">記録を綴じている……</div>
            </div>
          {:else if scene === "closed"}
            <div class="stage-center">
              <div class="closed-card" class:finale={campaignJustCompleted}>
                {#if campaignJustCompleted && activeCampaign}
                  <span class="finale-stamp">終 章</span>
                  <h2 class="finale-world">{activeCampaign.worldLabel}</h2>
                  <p class="finale-text">{activeCampaign.finale}</p>
                  <span class="finale-note">真相 100% —— このキャンペーンは完結した。</span>
                  <button class="primary-btn" onclick={backToChapters}>章一覧に戻る ▶</button>
                {:else}
                <span class="closed-stamp">{resolvedOk ? "事件解決" : "事件終了"}</span>
                <h2>{lastClosedTitle}</h2>
                {#if currentCase?.caseLogic && (currentCase.caseLogic.truth.trim() || currentCase.caseLogic.deductionChain.length)}
                  {@const cl = currentCase.caseLogic}
                  <div class="logic-panel">
                    <span class="logic-eyebrow">推理の再構成</span>
                    {#if questionText}<p class="logic-question">{questionText}</p>{/if}
                    {#if cl.deductionChain.length}
                      <ol class="logic-chain">
                        {#each cl.deductionChain as step}<li>{step}</li>{/each}
                      </ol>
                    {/if}
                    {#if cl.truth.trim()}<p class="logic-truth"><strong>真相</strong>{cl.truth}</p>{/if}
                    {#if cl.culprit.trim() || cl.motive.trim()}
                      <p class="logic-motive">
                        {#if cl.culprit.trim()}<strong>{cl.culprit}</strong>{/if}{#if cl.motive.trim()} — {cl.motive}{/if}
                      </p>
                    {/if}
                  </div>
                {/if}
                {#if currentCase?.metaBeat?.trim()}
                  <p class="meta-beat"><span>暗線</span>{currentCase.metaBeat}</p>
                {/if}
                {#if coveredPoints.length > 0}
                  <div class="coverage-panel">
                    <span class="cov-eyebrow">本章で扱った論点 ({coveredPoints.length})</span>
                    <ul>
                      {#each coveredPoints as p}
                        <li>
                          <span class="cov-mark" class:must={p.mustCover}>{p.mustCover ? "★" : "・"}</span>
                          <span class="cov-label">{p.label}</span>
                          {#if p.gist}<span class="cov-gist">— {p.gist}</span>{/if}
                        </li>
                      {/each}
                    </ul>
                  </div>
                {/if}
                {#if newRevelations.length > 0}
                  <div class="reveal-burst">
                    <span class="reveal-eyebrow">暗線が動いた —— 新たな真相</span>
                    {#each newRevelations as rev}
                      <div class="reveal-card">
                        <strong>{rev.title}</strong>
                        <p>{rev.reveal}</p>
                      </div>
                    {/each}
                  </div>
                {/if}
                <button class="primary-btn" onclick={backToChapters}>章一覧に戻る ▶</button>
                {/if}
              </div>
            </div>
          {/if}

          {#if error && scene !== "ai_error"}<div class="banner error stage-banner">{error}</div>{/if}
        </div>
      {/key}

      {#if recordOpen}
        <aside class="court-record" aria-label="証拠記録">
          <header>
            <h3>証拠記録</h3>
            <button class="ghost-btn small" onclick={cancelPresent} aria-label="閉じる">×</button>
          </header>
          <ul>
            {#each recordEvidence as item}
              {@const isPicked = item.id === presentedEvidenceId}
              <li>
                <button
                  class="record-item"
                  class:picked={isPicked}
                  onclick={() => pickEvidenceForPress(item.id)}
                >
                  <span class={`src-pill src-${item.sourceType}`}>{sourceLabel(item.sourceType)}</span>
                  <strong>{item.title}</strong>
                  <p>{item.excerpt}</p>
                </button>
              </li>
            {/each}
          </ul>
          <footer class="cr-footer">
            <button
              class="primary-btn wide"
              onclick={submitPresent}
              disabled={!presentedEvidenceId}
            >
              {presentedEvidenceId ? "異議あり！突きつける ▶" : "証拠を選んでください"}
            </button>
          </footer>
        </aside>
      {/if}
    </div>
  {/if}
</div>

<style>
  .detective {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    padding: 16px;
    color: var(--text-primary);
    background: var(--bg-primary);
    box-sizing: border-box;
  }

  .detective.trial { padding: 0; }
  h1, h2, h3, p { margin: 0; }

  /* ─── Common buttons ───────────────────────── */

  .ghost-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    border: 0.5px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: background 120ms ease, border-color 120ms ease;
  }
  .ghost-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .ghost-btn.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
  .ghost-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .primary-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 14px;
    border: 0;
    border-radius: 6px;
    background: var(--accent);
    color: var(--text-on-accent);
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
    transition: background 120ms ease;
  }
  .primary-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .primary-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ─── Banners ───────────────────────────────── */

  .banner {
    padding: 10px 14px;
    border: 0.5px solid var(--border);
    border-radius: 6px;
    background: var(--bg-card);
    color: var(--text-secondary);
    font-size: 13px;
    line-height: 1.55;
  }
  .banner.error {
    border-color: color-mix(in srgb, var(--red) 35%, var(--border));
    color: var(--red);
    background: rgba(255, 59, 48, 0.06);
  }

  .empty-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-items: center;
    padding: 36px;
    border: 0.5px dashed var(--border-strong);
    border-radius: 8px;
    background: var(--bg-card);
    text-align: center;
    color: var(--text-secondary);
  }
  .empty-card h3 { font-size: 16px; color: var(--text-primary); }

  /* ─── Case picker ──────────────────────────── */

  .case-pick {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    width: 100%;
    max-width: 560px;
    margin: 0 auto;
  }
  /* The title screen's main content column wraps the hero, world card, and CTA
     so they share ONE fixed visual width that adapts to narrow viewports. */
  .title-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    width: min(440px, 100%);
  }

  /* ─── Chapter list (per-course campaign) ───── */

  .chapter-screen {
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 620px;
    margin: 0 auto;
    width: 100%;
    padding: 18px 16px;
  }
  .ch-head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .ch-head-title {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    text-align: center;
  }
  .ch-head-title h1 {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.04em;
  }
  .ch-world {
    font-size: 11.5px;
    color: var(--accent);
    letter-spacing: 0.1em;
  }
  .ch-spacer { width: 64px; }
  .chapter-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chapter-row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
    border: 0.5px solid var(--border);
    border-radius: 11px;
    background: var(--bg-card);
    box-shadow: var(--shadow-sm);
    cursor: pointer;
    text-align: left;
    transition: transform 120ms ease, box-shadow 120ms ease, border-color 120ms ease;
  }
  .chapter-row:hover {
    transform: translateY(-1px);
    box-shadow: var(--shadow-md);
    border-color: var(--border-strong);
  }
  .chapter-row.locked {
    cursor: default;
    opacity: 0.55;
    background: var(--bg-tertiary);
    box-shadow: none;
  }
  .chapter-row.locked:hover {
    transform: none;
    box-shadow: none;
    border-color: var(--border);
  }
  .chapter-row.locked .ch-no { color: var(--text-tertiary); }
  .ch-no {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 13px;
    font-weight: 800;
    color: var(--kg-gold);
    letter-spacing: 0.04em;
    min-width: 48px;
  }
  .ch-main {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  .ch-main strong {
    font-size: 14px;
    font-weight: 650;
    color: var(--text-primary);
    line-height: 1.4;
  }
  .ch-status {
    font-size: 11.5px;
    color: var(--text-tertiary);
    letter-spacing: 0.04em;
  }
  .ch-go {
    font-size: 12px;
    font-weight: 700;
    color: var(--accent);
    white-space: nowrap;
  }

  /* Truth file — the unfolding 暗線 */
  .truth-file {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 6px;
    padding: 16px 18px;
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 34%, var(--border));
    border-radius: 12px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--kg-gold) 6%, transparent), transparent),
      var(--bg-card);
  }
  .tf-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }
  .tf-eyebrow {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.18em;
    color: var(--kg-gold);
  }
  .tf-progress {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .tf-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .tf-item {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }
  .tf-stage {
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--text-on-accent);
    font-size: 11px;
    font-weight: 800;
  }
  .tf-stage.locked {
    background: var(--bg-tertiary);
    color: var(--text-tertiary);
  }
  .tf-body { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .tf-body strong { font-size: 13px; color: var(--text-primary); font-weight: 700; }
  .tf-body p { font-size: 12.5px; color: var(--text-secondary); line-height: 1.7; }
  .tf-item.locked .tf-body strong { color: var(--text-tertiary); letter-spacing: 0.2em; }
  .tf-lock { color: var(--text-tertiary); font-size: 11.5px; }

  /* New-truth burst on chapter clear */
  /* Knowledge-point coverage panel — shown on close so the player can verify
     which testable points this chapter actually touched. */
  .coverage-panel {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    padding: 12px 14px;
    border: 0.5px solid var(--border);
    border-radius: 10px;
    background: var(--bg-card);
  }
  .cov-eyebrow {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.16em;
    color: var(--accent);
  }
  .coverage-panel ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .coverage-panel li {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-secondary);
  }
  .cov-mark {
    color: var(--text-tertiary);
    font-size: 11px;
    flex-shrink: 0;
  }
  .cov-mark.must { color: var(--kg-gold); font-weight: 700; }
  .cov-label { color: var(--text-primary); font-weight: 600; }
  .cov-gist { color: var(--text-tertiary); }

  /* Deduction reconstruction — the 明线 payoff on chapter clear. */
  .logic-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    padding: 14px 16px;
    border: 0.5px solid color-mix(in srgb, var(--green) 30%, var(--border));
    border-radius: 10px;
    background:
      linear-gradient(180deg, rgba(52, 199, 89, 0.05), transparent 60%),
      var(--bg-card);
    text-align: left;
  }
  .logic-eyebrow {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.16em;
    color: var(--green);
  }
  .logic-question {
    font-size: 13.5px;
    font-weight: 650;
    color: var(--text-primary);
    line-height: 1.6;
  }
  .logic-chain {
    list-style: decimal;
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .logic-chain li {
    font-size: 12.5px;
    line-height: 1.7;
    color: var(--text-secondary);
  }
  .logic-truth {
    font-size: 12.5px;
    line-height: 1.7;
    color: var(--text-primary);
  }
  .logic-truth strong {
    display: inline-block;
    margin-right: 8px;
    padding: 1px 8px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--green) 16%, transparent);
    color: var(--green);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    vertical-align: middle;
  }
  .logic-motive {
    font-size: 12.5px;
    line-height: 1.65;
    color: var(--text-secondary);
  }
  .logic-motive strong { color: var(--text-primary); }

  /* What this chapter advanced in the 暗线 — a quiet badge line. */
  .meta-beat {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-secondary);
    text-align: left;
  }
  .meta-beat span {
    flex-shrink: 0;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--kg-gold-light);
    color: var(--kg-gold);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.1em;
  }

  .reveal-burst {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    padding: 14px 16px;
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 50%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--kg-gold) 8%, var(--bg-card));
    animation: pop-in 320ms ease both;
  }
  .reveal-eyebrow {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--kg-gold);
    text-align: center;
  }
  .reveal-card { display: flex; flex-direction: column; gap: 3px; }
  .reveal-card strong { font-size: 13.5px; color: var(--text-primary); }
  .reveal-card p { font-size: 12.5px; color: var(--text-secondary); line-height: 1.75; }

  /* Campaign finale (chapter-list panel + close-screen ending) */
  .finale-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 20px;
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 50%, transparent);
    border-radius: 12px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--kg-gold) 10%, transparent), transparent),
      var(--bg-card);
  }
  .finale-stamp {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 26px;
    font-weight: 800;
    letter-spacing: 0.3em;
    color: var(--kg-gold);
  }
  .finale-stamp.small { font-size: 18px; }
  .finale-world {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 18px;
    font-weight: 700;
    color: var(--accent);
    letter-spacing: 0.08em;
    text-align: center;
  }
  .finale-text {
    font-size: 14px;
    line-height: 2;
    color: var(--text-primary);
    text-align: center;
    white-space: pre-line;
    max-width: 520px;
  }
  .finale-note {
    font-size: 11.5px;
    color: var(--text-tertiary);
    letter-spacing: 0.08em;
  }
  .closed-card.finale {
    gap: 14px;
    border-color: color-mix(in srgb, var(--kg-gold) 50%, transparent);
  }

  /* ─── Course selection modal ───────────────── */

  /* No dimming backdrop — just an invisible click-catcher that centers the
     floating popup. The popup itself reads as elevated via a soft shadow. */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    background: transparent;
  }
  .modal {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(440px, 100%);
    max-height: min(640px, 86vh);
    padding: 18px 20px 20px;
    border: 0.5px solid var(--border);
    border-radius: 14px;
    /* Opaque surface — must not show the page behind it. */
    background: var(--bg-primary);
    box-shadow:
      0 1px 2px rgba(0, 0, 0, 0.05),
      0 8px 24px rgba(0, 0, 0, 0.12),
      0 24px 64px rgba(0, 0, 0, 0.10);
    animation: modal-in 200ms cubic-bezier(.2, 1, .4, 1) both;
  }
  @keyframes modal-in {
    from { opacity: 0; transform: translateY(8px) scale(0.98); }
    to { opacity: 1; transform: translateY(0) scale(1); }
  }
  .modal-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .modal-head h2 {
    font-size: 17px;
    font-weight: 750;
    color: var(--text-primary);
  }
  .modal-foot {
    padding-top: 4px;
  }
  .modal-foot .primary-btn.wide {
    width: 100%;
  }

  /* ─── Selection list ───────────────────────── */

  .course-search {
    width: 100%;
    height: 34px;
    padding: 0 12px;
    border: 0.5px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-input);
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
  }
  .course-search:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-light);
  }

  .select-list {
    list-style: none;
    margin: 0;
    padding: 2px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }
  .select-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 12px 14px;
    border: 0.5px solid var(--border);
    border-radius: 8px;
    background: var(--bg-card);
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms ease, background 120ms ease;
  }
  .select-row:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--bg-hover);
  }
  .select-row.on {
    border-color: var(--accent);
    background: var(--accent-light);
  }
  .select-row:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .sr-name {
    flex: 1;
    min-width: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sr-check {
    flex-shrink: 0;
    color: var(--accent);
    font-size: 15px;
    font-weight: 800;
    line-height: 1;
  }
  .sr-empty {
    flex-shrink: 0;
    color: var(--text-tertiary);
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--bg-tertiary);
  }
  .primary-btn.wide {
    min-width: 240px;
    height: 38px;
    font-size: 14px;
  }

  /* ─── Title screen (home / session start) ─── */

  .title-screen {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 22px;
    max-width: 460px;
    margin: 0 auto;
    padding: clamp(24px, 6vh, 64px) 8px 32px;
    text-align: center;
  }

  .ts-hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .ts-emblem {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    position: relative;
  }
  .ts-logo {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .ts-title {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 32px;
    font-weight: 800;
    letter-spacing: 0.14em;
    color: var(--text-primary);
    margin-left: 0.14em;
  }
  .ts-tagline {
    color: var(--text-tertiary);
    font-size: 12px;
    letter-spacing: 0.08em;
  }

  /* Campaign world banner — fixed-size card so it doesn't reshape itself based
     on tagline length; shrinks down for narrow viewports via min(). */
  .ts-world {
    width: 100%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 16px 20px;
    border-radius: 14px;
    background: var(--bg-card);
    border: 1px solid color-mix(in srgb, var(--kg-gold) 36%, var(--border));
    box-shadow: var(--shadow-sm);
  }
  .ts-world-tagline {
    max-width: 100%;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .ts-world-label {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--accent);
  }
  .ts-world-tagline {
    margin: 0;
    font-size: 12.5px;
    color: var(--text-secondary);
    letter-spacing: 0.04em;
    text-align: center;
  }
  .ts-world-meter {
    width: 100%;
    height: 5px;
    border-radius: 3px;
    background: var(--bg-tertiary);
    overflow: hidden;
  }
  .ts-world-fill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: linear-gradient(90deg, var(--kg-gold), var(--kg-gold-light));
    transition: width 240ms ease;
  }
  .ts-world-meta {
    font-size: 11px;
    color: var(--text-tertiary);
    letter-spacing: 0.06em;
  }

  /* Campaign switcher (when more than one course is active) */
  .ts-campaign-switch {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
  }
  .campaign-chip {
    padding: 5px 12px;
    border-radius: 999px;
    border: 0.5px solid var(--border-strong);
    background: var(--bg-card);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 120ms ease;
  }
  .campaign-chip.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
  /* The one dominant call to action — fixed size so it doesn't fight the
     centered visual hierarchy. */
  .ts-start {
    width: 100%;
    box-sizing: border-box;
    min-height: 84px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 4px;
    padding: 18px 22px;
    border: 0;
    border-radius: 14px;
    background: linear-gradient(135deg, var(--accent), var(--accent-hover));
    color: var(--text-on-accent);
    cursor: pointer;
    box-shadow: var(--shadow-lg);
    transition: transform 140ms ease, box-shadow 140ms ease;
  }
  .ts-start:hover {
    transform: translateY(-2px);
    box-shadow: 0 12px 32px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .ts-start-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }
  .ts-start-main {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 19px;
    font-weight: 800;
    letter-spacing: 0.1em;
  }
  .ts-start-play {
    font-size: 13px;
    opacity: 0.85;
  }
  .ts-start-sub {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.82);
    font-feature-settings: "tnum" 1;
  }

  .ts-sep { opacity: 0.5; }

  /* Bottom-right pill: passive stats on the left + interactive edit on the
     right, joined as one visual unit. Only the edit half is clickable. */
  .title-corner {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 12;
    display: inline-flex;
    align-items: stretch;
    border-radius: 999px;
    border: 0.5px solid var(--border-strong);
    background: var(--bg-card);
    box-shadow: var(--shadow-md);
    overflow: hidden;
  }
  .tc-stats {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    color: var(--text-tertiary);
    font-size: 12px;
    letter-spacing: 0.02em;
    user-select: none;
    cursor: default;
  }
  .tc-stats b {
    color: var(--accent);
    font-weight: 700;
    font-feature-settings: "tnum" 1;
  }
  .tc-sep { opacity: 0.5; }
  .tc-div {
    width: 0.5px;
    background: var(--border-strong);
    align-self: stretch;
  }
  .tc-edit {
    border: 0;
    background: transparent;
    padding: 8px 14px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
  }
  .tc-edit:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ─── Trial stage ──────────────────────────── */

  .stage {
    display: flex;
    flex-direction: column;
    flex: 1;
    position: relative;
    overflow: hidden;
    min-height: 100%;
  }

  .stage-frame {
    display: flex;
    flex-direction: column;
    flex: 1;
    height: 100%;
    min-height: calc(100vh - 80px);
    /* top padding makes room for floating corner controls */
    padding: 60px 16px 16px;
    gap: 14px;
    position: relative;
  }

  .stage-frame.shake-1 { animation: stage-shake 480ms ease-out; }
  @keyframes stage-shake {
    0% { transform: translate(0, 0); }
    15% { transform: translate(-8px, 3px); }
    30% { transform: translate(7px, -2px); }
    45% { transform: translate(-5px, 2px); }
    60% { transform: translate(4px, -1px); }
    75% { transform: translate(-2px, 0); }
    100% { transform: translate(0, 0); }
  }

  /* ─── Floating corner controls (no top bar) ─── */

  /* Base — used by both record (absolutely positioned top-left) and close
     (lives in the .corner-right flex row). */
  .corner-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 34px;
    padding: 0 14px;
    border: 0.5px solid var(--border-strong);
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 12.5px;
    font-weight: 650;
    cursor: pointer;
    box-shadow: var(--shadow-sm);
    transition: background 120ms ease, transform 120ms ease;
  }
  .corner-btn:hover {
    background: var(--bg-hover);
    transform: translateY(-1px);
  }
  .corner-btn.record {
    position: absolute;
    top: 12px;
    left: 14px;
    z-index: 20;
    border-color: color-mix(in srgb, var(--accent) 24%, var(--border-strong));
    background: var(--accent-light);
    color: var(--accent);
  }
  .corner-btn.record i {
    font-style: normal;
    font-size: 15px;
    line-height: 1;
    font-weight: 800;
  }
  .corner-btn.close {
    width: 34px;
    padding: 0;
    font-size: 18px;
    line-height: 1;
    color: var(--text-secondary);
  }

  .corner-right {
    position: absolute;
    top: 12px;
    right: 14px;
    z-index: 20;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex-wrap: nowrap;
  }

  .lives {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border-radius: 999px;
    background: var(--kg-gold-light);
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 28%, transparent);
    color: var(--kg-gold);
    box-shadow: var(--shadow-sm);
  }
  .lives i {
    font-style: normal;
    font-size: 14px;
    line-height: 1;
    color: var(--kg-gold);
  }
  .lives i.lost { color: var(--text-tertiary); opacity: 0.45; }

  .lie-counter {
    padding: 5px 11px;
    border-radius: 999px;
    background: var(--accent-light);
    border: 0.5px solid color-mix(in srgb, var(--accent) 28%, transparent);
    color: var(--accent);
    font-family: ui-monospace, monospace;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    box-shadow: var(--shadow-sm);
  }

  .act-counter {
    padding: 5px 11px;
    border-radius: 999px;
    background: var(--bg-card);
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 40%, var(--border));
    color: var(--kg-gold);
    font-family: ui-monospace, monospace;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    box-shadow: var(--shadow-sm);
  }

  /* Quiet 暗线 marker: a chapter act carrying a hidden-thread clue. */
  .meta-hint {
    padding: 5px 11px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--kg-gold) 12%, var(--bg-card));
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 34%, transparent);
    color: var(--kg-gold);
    font-size: 11px;
    font-weight: 650;
    letter-spacing: 0.04em;
    box-shadow: var(--shadow-sm);
    animation: pulse 2.4s ease-in-out infinite;
  }

  /* ─── Act intro (story card) ──────────────────────────── */

  .phase-act-intro {
    max-width: 640px;
    width: 100%;
    margin: 0 auto;
    justify-content: center;
  }
  .act-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 26px 26px 28px;
    border: 0.5px solid var(--border);
    border-radius: 14px;
    background: var(--bg-card);
    box-shadow: var(--shadow-lg);
  }
  /* Case-file briefing header on the first act intro. */
  .case-file {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 14px;
    margin-bottom: 4px;
    border: 0.5px solid color-mix(in srgb, var(--kg-gold) 32%, var(--border));
    border-radius: 9px;
    background: color-mix(in srgb, var(--kg-gold) 6%, var(--bg-card));
  }
  .case-file-tag {
    align-self: flex-start;
    padding: 1px 9px;
    border-radius: 999px;
    background: var(--kg-gold-light);
    color: var(--kg-gold);
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
  }
  .case-file p {
    color: var(--text-secondary);
    font-size: 12.5px;
    line-height: 1.75;
  }
  .act-prologue {
    color: var(--text-secondary);
    font-size: 13.5px;
    line-height: 1.9;
    white-space: pre-line;
    padding-bottom: 12px;
    margin-bottom: 4px;
    border-bottom: 0.5px solid var(--border);
  }
  .act-eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--accent);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.16em;
  }
  .act-seed {
    color: var(--kg-gold);
    font-size: 10px;
  }
  .act-title {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 25px;
    font-weight: 800;
    line-height: 1.35;
    color: var(--text-primary);
    letter-spacing: 0.04em;
  }
  .act-location {
    align-self: flex-start;
    padding: 3px 10px;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-tertiary);
    font-size: 11.5px;
    letter-spacing: 0.06em;
  }
  .act-narrative {
    color: var(--text-primary);
    font-size: 15px;
    line-height: 1.95;
    white-space: pre-line;
  }

  /* ─── Act clear (brief between-act cut-in) ────────────── */

  .act-clear-overlay {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    flex: 1;
    width: 100%;
    animation: actClearIn 180ms ease-out;
  }
  .act-clear-overlay .cutin-text {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 34px;
    font-weight: 800;
    color: var(--accent);
    letter-spacing: 0.06em;
    transform: rotate(-3deg);
  }
  .act-clear-overlay p {
    color: var(--text-secondary);
    font-size: 13.5px;
    letter-spacing: 0.08em;
  }
  @keyframes actClearIn {
    from { opacity: 0; transform: scale(0.94); }
    to { opacity: 1; transform: scale(1); }
  }

  /* ─── Stage center (loading / error / result / closed) ─── */

  .stage-center {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    flex: 1;
    padding: 20px 12px;
  }

  .stage-banner { margin-top: 8px; }

  /* ─── Phase frames (investigate / cross_exam) ─────────── */

  .phase {
    display: flex;
    flex-direction: column;
    flex: 1;
    gap: 14px;
    min-height: 0;
  }

  /* Investigation phase */
  .phase-investigate {
    max-width: 760px;
    width: 100%;
    margin: 0 auto;
  }

  .scenario-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 18px 22px;
    border: 0.5px solid var(--border);
    border-radius: 10px;
    background: var(--bg-card);
    box-shadow: var(--shadow-card);
  }
  .scenario-eyebrow {
    color: var(--accent);
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.22em;
    text-transform: uppercase;
  }
  .scenario-title {
    font-size: 18px;
    font-weight: 700;
    line-height: 1.4;
    color: var(--text-primary);
    letter-spacing: 0.02em;
  }
  .scenario-body {
    color: var(--text-primary);
    font-size: 14.5px;
    line-height: 1.85;
    white-space: pre-line;
  }

  .evidence-block {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px 18px 14px;
    border: 0.5px solid var(--border);
    border-radius: 10px;
    background: var(--bg-card);
    box-shadow: var(--shadow-card);
  }
  .evidence-viewer {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border: 0.5px solid var(--border);
    border-radius: 8px;
    background: var(--bg-input);
    min-height: 140px;
  }
  .ev-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .ev-title {
    font-size: 15px;
    line-height: 1.4;
    color: var(--text-primary);
    font-weight: 650;
  }
  .ev-body {
    color: var(--text-secondary);
    font-size: 13.5px;
    line-height: 1.75;
    white-space: pre-line;
  }
  .evidence-nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .evidence-dots {
    display: inline-flex;
    gap: 6px;
  }
  .evidence-dots i {
    width: 6px;
    height: 6px;
    border-radius: 999px;
    background: var(--border-strong);
    transition: background 120ms ease;
  }
  .evidence-dots i.on {
    background: var(--accent);
  }

  .phase-footer {
    display: flex;
    justify-content: center;
    padding-top: 6px;
  }

  /* Cross-examination phase — persistent layout */
  .phase-cross-exam {
    max-width: 760px;
    width: 100%;
    margin: 0 auto;
  }

  /* ─── Cross-exam: trial-panel (witness + textbox + actions in one box) ─── */

  .trial-panel {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 22px 22px 18px;
    border: 0.5px solid var(--border);
    border-radius: 14px;
    background:
      radial-gradient(circle at 50% -10%, color-mix(in srgb, var(--kg-gold) 8%, transparent), transparent 60%),
      var(--bg-card);
    box-shadow: var(--shadow-md);
    transition: border-color 220ms ease;
    width: min(680px, 100%);
    margin: 0 auto;
  }
  .trial-panel.pressed {
    border-color: color-mix(in srgb, var(--accent) 28%, var(--border));
  }

  /* Two-character stage: detective (left) faces witness (right). Both are
     illustration slots — `.cast-art` is the placeholder until art is dropped
     in via background-image keyed by [data-character][data-pose]. */
  .cast-row {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 12px;
    border-bottom: 0.5px solid var(--border);
  }
  .cast {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    margin: 0;
    /* Without this, the flex default min-height:auto lets each cast figure's
       automatic minimum balloon to the full stage height, stretching .cast-row
       (and the whole trial-panel) and pushing the testimony + actions off
       screen. Pin it so the figures size to their content. */
    min-height: 0;
  }
  .cast-art {
    width: 64px;
    height: 64px;
    border-radius: 12px;
    display: grid;
    place-items: center;
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-weight: 700;
    font-size: 26px;
    color: var(--text-on-accent);
    background-size: cover;
    background-position: center top;
  }
  /* Placeholder palette per character (replaced by real art later) */
  .cast.detective .cast-art {
    background: linear-gradient(135deg, var(--accent), var(--accent-hover));
  }
  .cast.witness .cast-art {
    background: linear-gradient(135deg, var(--kg-gold), color-mix(in srgb, var(--kg-gold) 50%, var(--accent)));
  }
  .cast figcaption {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
  }
  .cast figcaption strong {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 13px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.04em;
  }
  .cast figcaption span,
  .cast-pose {
    color: var(--text-tertiary);
    font-size: 10.5px;
    font-weight: 600;
  }
  /* Dev hint: which detective pose the illustration slot expects right now */
  .cast-pose {
    font-family: ui-monospace, monospace;
    letter-spacing: 0.08em;
    color: var(--accent);
  }

  /* ─── Textbox (testimony / press response) ───── */

  .textbox {
    position: relative;
    width: 100%;
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 20px 24px;
    border: 0.5px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-input);
    color: var(--text-primary);
    cursor: pointer;
    box-shadow: var(--shadow-sm);
    transition: border-color 120ms ease, background 220ms ease;
    min-height: 100px;
  }
  .textbox.pressed {
    background:
      linear-gradient(180deg, var(--accent-light), transparent 60%),
      var(--bg-input);
  }
  .textbox:hover { border-color: var(--accent); }
  /* Mode label replaces the old colored left bar. */
  .textbox-tag {
    font-family: ui-monospace, monospace;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.16em;
    color: var(--kg-gold);
  }
  .textbox.pressed .textbox-tag { color: var(--accent); }

  /* ─── Action row (anchored inside trial-panel) ──── */

  .trial-actions {
    display: grid;
    grid-template-columns: auto 1fr 1fr auto;
    align-items: center;
    gap: 8px;
  }
  .nav-arrow {
    width: 38px;
    height: 38px;
    border: 0.5px solid var(--border-strong);
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 15px;
    cursor: pointer;
    transition: background 120ms ease, transform 120ms ease;
    box-shadow: var(--shadow-sm);
  }
  .nav-arrow:hover:not(:disabled) {
    background: var(--bg-hover);
    transform: scale(1.04);
  }
  .nav-arrow:disabled { opacity: 0.35; cursor: default; }

  .trial-actions .action-btn { padding: 11px 14px; }
  .trial-actions .action-btn:disabled {
    opacity: 0.45;
    cursor: default;
    transform: none;
    box-shadow: var(--shadow-sm);
  }

  .textbox-text {
    font-size: 16px;
    line-height: 1.85;
    color: var(--text-primary);
    margin: 0;
  }
  .textbox-text.testimony {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    color: color-mix(in srgb, var(--accent) 70%, var(--text-primary));
    font-style: italic;
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .textbox-text.testimony .hi {
    background: transparent;
    color: var(--kg-gold);
    font-style: normal;
    font-weight: 800;
    border-bottom: 2px solid var(--kg-gold);
    padding: 0 1px;
  }
  .textbox-text.reply {
    font-style: normal;
    color: var(--text-primary);
  }

  .textbox-advance {
    position: absolute;
    right: 16px;
    bottom: 8px;
    color: var(--accent);
    font-size: 14px;
    line-height: 1;
    animation: bob 1.2s ease-in-out infinite;
  }
  @keyframes bob {
    0%, 100% { transform: translateY(0); opacity: 0.55; }
    50%      { transform: translateY(2px); opacity: 1; }
  }

  /* Fail overlay (brief, in-place) */
  .fail-overlay {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--red);
    animation: fail-flash 0.3s ease;
  }
  @keyframes fail-flash {
    0% { transform: scale(0.96); opacity: 0.4; }
    100% { transform: scale(1); opacity: 1; }
  }
  .fail-overlay .dameda {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 48px;
    font-weight: 800;
    color: var(--red);
    letter-spacing: 0.06em;
  }
  .fail-overlay p {
    color: var(--text-secondary);
    font-size: 13px;
    font-feature-settings: "tnum" 1;
  }

  /* Court Record footer (the "突きつける!" submit row) */
  .cr-footer {
    padding-top: 8px;
    border-top: 0.5px solid var(--border);
    display: flex;
  }
  .cr-footer .primary-btn.wide { width: 100%; }

  .loading-card,
  .error-card,
  .result-pass,
  .unresolved-card,
  .closing-card,
  .closed-card {
    background: var(--bg-card);
    border: 0.5px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow-md);
  }

  /* Loading */
  .loading-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 28px;
    max-width: 420px;
  }
  .loading-spinner {
    width: 24px;
    height: 24px;
    border-radius: 999px;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }
  .loading-card p { color: var(--text-primary); font-size: 14px; font-weight: 600; line-height: 1.6; text-align: center; }
  .loading-sub { color: var(--text-tertiary); font-size: 11.5px; letter-spacing: 0.04em; text-align: center; }

  /* Error */
  .error-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 28px 32px;
    max-width: 600px;
    border-color: color-mix(in srgb, var(--red) 35%, var(--border));
  }
  .error-msg {
    color: var(--text-primary);
    font-size: 14px;
    line-height: 1.55;
    text-align: center;
    white-space: pre-line;
  }
  .error-sub { color: var(--text-secondary); font-size: 12.5px; text-align: center; }
  .error-actions { display: flex; gap: 10px; margin-top: 4px; }

  @keyframes pop-in {
    from { opacity: 0; transform: translateY(6px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes slide-up {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
  }

  /* Cross-examination action buttons (used inside .trial-actions) */
  .action-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 14px 14px;
    border: 0;
    border-radius: 8px;
    background: var(--bg-input);
    color: var(--text-primary);
    cursor: pointer;
    transition: transform 120ms ease, background 120ms ease, box-shadow 120ms ease;
    box-shadow: var(--shadow-sm);
  }
  .action-btn strong {
    font-size: 15px;
    font-weight: 750;
    letter-spacing: 0.04em;
  }
  .action-btn span {
    color: var(--text-tertiary);
    font-size: 11.5px;
  }
  .action-btn:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: var(--shadow-md);
  }
  .action-btn.press {
    background: var(--kg-gold-light);
    color: var(--kg-gold);
  }
  .action-btn.press strong { color: var(--kg-gold); }
  .action-btn.press:hover:not(:disabled) {
    background: color-mix(in srgb, var(--kg-gold) 16%, var(--bg-card));
  }
  .action-btn.present {
    background: var(--accent);
    color: var(--text-on-accent);
  }
  .action-btn.present strong { color: var(--text-on-accent); }
  .action-btn.present span { color: rgba(255, 255, 255, 0.72); }
  .action-btn.present:hover:not(:disabled) { background: var(--accent-hover); }

  .ghost-btn.small {
    height: 26px;
    padding: 0 10px;
    font-size: 11.5px;
  }


  /* Result pass — with OBJECTION cut-in */
  .result-stack {
    flex-direction: column;
    gap: 14px;
    align-items: center;
  }
  .objection-cutin {
    width: min(680px, 100%);
    padding: 24px 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    background:
      radial-gradient(circle at 50% 50%, rgba(255, 59, 48, 0.18), transparent 70%),
      var(--bg-card);
    border: 0.5px solid color-mix(in srgb, var(--red) 30%, transparent);
    border-radius: 10px;
    overflow: hidden;
    animation: cutin-in 480ms cubic-bezier(.2, 1.4, .4, 1) both;
  }
  .cutin-text {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 44px;
    font-weight: 900;
    color: var(--red);
    letter-spacing: 0.06em;
    text-shadow: 2px 2px 0 rgba(0, 0, 0, 0.12);
    transform: rotate(-3deg);
    animation: cutin-pulse 1.2s ease-in-out infinite;
  }
  @keyframes cutin-in {
    0%   { opacity: 0; transform: scale(0.6); }
    60%  { opacity: 1; transform: scale(1.08); }
    100% { opacity: 1; transform: scale(1); }
  }
  @keyframes cutin-pulse {
    0%, 100% { transform: rotate(-3deg) scale(1); }
    50%      { transform: rotate(-3deg) scale(1.04); }
  }
  .result-pass {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    width: min(680px, 100%);
    padding: 22px 24px;
    border: 0.5px solid var(--green);
    background:
      linear-gradient(180deg, rgba(52, 199, 89, 0.06), rgba(52, 199, 89, 0.02)),
      var(--bg-card);
    animation: pop-in 360ms 360ms ease both;
  }
  .rp-stamp {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 22px;
    font-weight: 800;
    letter-spacing: 0.14em;
    color: var(--green);
  }
  .rp-lie {
    color: var(--text-secondary);
    font-size: 13.5px;
    text-align: center;
  }
  .rp-progress {
    color: var(--text-tertiary);
    font-size: 12px;
    letter-spacing: 0.06em;
  }
  .rp-evidence {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    padding: 12px 14px;
    border: 0.5px solid var(--border);
    border-radius: 8px;
    background: var(--bg-input);
  }
  .rp-evidence strong { font-size: 13.5px; color: var(--text-primary); font-weight: 650; }
  .rp-evidence p { color: var(--text-secondary); font-size: 12.5px; line-height: 1.55; }

  /* Result fail */
  .result-fail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 28px 28px;
    border: 0.5px solid var(--red);
    background:
      linear-gradient(180deg, rgba(255, 59, 48, 0.08), rgba(255, 59, 48, 0.02)),
      var(--bg-card);
    animation: fail-flash 0.35s ease;
  }
  @keyframes fail-flash {
    0% { background-color: rgba(255, 59, 48, 0.2); }
    100% { background-color: rgba(255, 59, 48, 0.02); }
  }
  .dameda {
    font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
    font-size: 36px;
    font-weight: 800;
    color: var(--red);
    letter-spacing: 0.06em;
  }
  .result-fail p { color: var(--text-secondary); font-size: 13px; font-feature-settings: "tnum" 1; }

  /* Unresolved */
  .unresolved-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 28px 28px;
  }
  .unresolved-stamp {
    color: var(--text-secondary);
    font-size: 22px;
    font-weight: 700;
    letter-spacing: 0.24em;
  }
  .unresolved-card p { color: var(--text-secondary); font-size: 13px; }

  /* Closing / Closed */
  .closing-card {
    padding: 28px 28px;
    color: var(--text-tertiary);
    font-size: 13px;
    letter-spacing: 0.08em;
    text-align: center;
  }
  .closed-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 28px 28px;
    border: 0.5px solid var(--green);
    background:
      linear-gradient(180deg, rgba(52, 199, 89, 0.06), rgba(52, 199, 89, 0.02)),
      var(--bg-card);
    animation: pop-in 360ms ease both;
  }
  .closed-stamp {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: 0.18em;
    color: var(--green);
    padding: 4px 14px;
    border: 0.5px solid var(--green);
    border-radius: 6px;
  }
  .closed-card h2 { font-size: 15px; color: var(--text-primary); text-align: center; font-weight: 650; }
  .closed-card p { color: var(--text-secondary); font-size: 12.5px; }

  @keyframes pulse {
    0%, 100% { opacity: 0.55; }
    50% { opacity: 1; }
  }

  /* ─── Source pills ─────────────────────────── */

  .src-pill {
    padding: 1px 8px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
  }
  .src-live   { background: var(--accent-light); color: var(--accent); }
  .src-signal { background: var(--kg-gold-light); color: var(--kg-gold); }
  .src-doubt  { background: rgba(175, 82, 222, 0.1); color: var(--purple); }

  /* ─── Court Record ─────────────────────────── */

  .court-record {
    position: absolute;
    top: 70px;
    right: 14px;
    bottom: 14px;
    width: min(380px, 92vw);
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    border: 0.5px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-card);
    color: var(--text-primary);
    box-shadow: var(--shadow-lg);
    z-index: 10;
    animation: slide-in 200ms ease both;
  }
  @keyframes slide-in {
    from { transform: translateX(16px); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  .court-record header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-bottom: 8px;
    border-bottom: 0.5px solid var(--border);
  }
  .court-record header h3 {
    font-size: 13.5px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--text-primary);
  }
  .court-record ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
  }
  .record-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 5px;
    width: 100%;
    padding: 10px 12px;
    border: 0.5px solid var(--border);
    border-radius: 8px;
    background: var(--bg-input);
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms ease, transform 120ms ease;
  }
  .record-item:hover {
    border-color: var(--accent);
    transform: translateX(-2px);
  }
  .record-item .src-pill {
    align-self: flex-start;
  }
  .record-item.picked {
    border-color: var(--accent);
    border-width: 1px;
    background: var(--accent-light);
  }
  .record-item strong {
    font-size: 13px;
    line-height: 1.4;
    color: var(--text-primary);
    font-weight: 650;
  }
  .record-item p {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.5;
  }

  /* ─── Responsive ───────────────────────────── */

  @media (max-width: 720px) {
    .corner-btn.record span { display: none; }
    .corner-btn.record {
      width: 34px;
      padding: 0;
      justify-content: center;
    }
    .lie-counter {
      font-size: 10.5px;
      padding: 4px 8px;
    }
    .court-record {
      top: 56px;
      right: 8px;
      left: 8px;
      width: auto;
    }
    .trial-actions {
      grid-template-columns: auto 1fr auto;
      grid-template-rows: auto auto;
      row-gap: 6px;
    }
    .trial-actions .action-btn.press { grid-column: 2 / 3; grid-row: 1; }
    .trial-actions .action-btn.present { grid-column: 2 / 3; grid-row: 2; }
    .trial-actions .nav-arrow:first-child { grid-row: 1 / 3; }
    .trial-actions .nav-arrow:last-child { grid-row: 1 / 3; }
  }
</style>
