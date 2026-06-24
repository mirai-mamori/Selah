export type CourseAgentDetailKey =
  | "pending"
  | "seat"
  | "print"
  | "organize"
  | "standing"
  | "documents"
  | "log";

export type CourseAgentModuleKey =
  | "control"
  | "highlight"
  | "pending"
  | "standing"
  | "seat"
  | "print"
  | "organize"
  | "documents"
  | "log"
  | "navigation";

export const COURSE_AGENT_MODULE_KEYS: readonly CourseAgentModuleKey[] = [
  "control",
  "highlight",
  "pending",
  "standing",
  "seat",
  "print",
  "organize",
  "documents",
  "log",
  "navigation",
] as const;

export const COURSE_AGENT_DETAIL_TITLES: Record<CourseAgentDetailKey, string> = {
  pending: "保留中",
  seat: "座席",
  print: "印刷",
  organize: "整理",
  standing: "記憶",
  documents: "分析した資料",
  log: "ログ",
};

export const COURSE_AGENT_MODULE_CONTRACTS: ReadonlyArray<{
  key: CourseAgentModuleKey;
  title: string;
  displays: readonly string[];
  inputs: readonly string[];
}> = [
  {
    key: "control",
    title: "状態",
    displays: ["stage/progress", "error tone", "manual refresh"],
    inputs: ["running", "stage", "totalDocuments", "processedDocuments", "lastError"],
  },
  {
    key: "highlight",
    title: "要点",
    displays: ["current overall summary", "last checked", "attention tone"],
    inputs: ["analysis.summary", "lastRun", "documentAnalyses.triggerDecision"],
  },
  {
    key: "pending",
    title: "保留中",
    displays: ["non-TODO notices", "shortcuts to blocked module actions"],
    inputs: ["analysis.findings", "itemStates", "printResults", "pendingSeatNotification", "currentDate"],
  },
  {
    key: "standing",
    title: "記憶",
    displays: ["active long-lived context", "expired archived context"],
    inputs: ["analysis.standingContext", "analysis.archivedContext", "currentDate"],
  },
  {
    key: "seat",
    title: "座席",
    displays: ["current assignment", "confidence", "evidence"],
    inputs: ["analysis.seat", "pendingSeatNotification"],
  },
  {
    key: "print",
    title: "印刷",
    displays: ["print results", "approval waits", "candidate reasons"],
    inputs: ["analysis.printCandidates", "printResults"],
  },
  {
    key: "organize",
    title: "整理",
    displays: ["thematic file groups the agent auto-filed", "undo last organize"],
    inputs: ["organizeGroups", "organizeCanUndo", "lastOrganized"],
  },
  {
    key: "documents",
    title: "分析した資料",
    displays: ["per-document summaries", "pending summary count"],
    inputs: ["documentAnalyses", "pendingSummaryIds", "sourceEvents"],
  },
  {
    key: "log",
    title: "ログ",
    displays: ["recent run log", "current error"],
    inputs: ["runLog", "lastError"],
  },
  {
    key: "navigation",
    title: "入口",
    displays: ["visible module pills", "detail counts"],
    inputs: ["pending", "seat", "standing", "print", "documents", "log"],
  },
] as const;

export interface CourseAutomationConfig {
  enabled: boolean;
  intervalMinutes: number;
}

export interface CourseItemFlags {
  action?: boolean;
  urgent?: boolean;
}

export interface CourseAgentItem {
  id: string;
  facet?: string;
  text: string;
  category?: string;
  expiresAt?: string;
  flags?: CourseItemFlags;
}

export interface CourseArchivedGroup {
  label: string;
  items: string[];
}

export interface CourseSeatConclusion {
  assignment: string;
  evidence: string[];
  confidence: number;
}

export interface CoursePrintCandidate {
  filename: string;
  reason: string;
  confidence: number;
  category?: string;
}

export interface CoursePrintResult {
  actionKey?: string;
  filename: string;
  path?: string;
  status: string;
  detail: string;
  category?: string;
}

export interface CourseDocumentAnalysis {
  id: string;
  fingerprint?: string;
  sourceFingerprint?: string;
  kind: string;
  title: string;
  filename: string;
  path?: string;
  status: string;
  content?: string;
  summary: string;
  findings?: string[];
  seatEvidence?: string[];
  printInstruction?: string;
  triggerDecision: string;
  observationContext: string;
  error: string;
}

export interface CourseOrganizeFile {
  filename: string;
  title?: string;
  fromPath?: string;
  toPath?: string;
  moved?: boolean;
}

export interface CourseOrganizeGroup {
  id: string;
  label: string;
  kind?: string;
  folder?: string;
  files: CourseOrganizeFile[];
}

export interface CourseSourceEvent {
  id: string;
  event: string;
  documentId: string;
  kind: string;
  title: string;
  filename: string;
  previousSummary?: string;
  detail?: string;
  attention?: boolean;
}

export interface CourseRunLogEntry {
  at: number;
  level: string;
  message: string;
}

export interface CourseAutomationAnalysis {
  summary: string;
  findings: CourseAgentItem[];
  standingContext: CourseAgentItem[];
  archivedContext?: CourseArchivedGroup[];
  seat: CourseSeatConclusion;
  printCandidates: CoursePrintCandidate[];
}

export interface CourseAutomationStatus {
  lunaId: string;
  courseName?: string;
  running: boolean;
  stage: string;
  lastRun?: number;
  lastOk?: boolean;
  lastError: string;
  trigger?: string;
  fingerprint?: string;
  downloadedFiles: string[];
  externalLinks: string[];
  totalDocuments: number;
  processedDocuments: number;
  currentDocument: string;
  documentAnalyses: CourseDocumentAnalysis[];
  pendingSummaryIds: string[];
  sourceEvents?: CourseSourceEvent[];
  pendingSourceEventIds?: string[];
  lastSummaryDocumentIds?: string[];
  pendingNotificationIds?: string[];
  notifiedDocumentIds?: string[];
  pendingSeatNotification?: boolean;
  lastNotifiedSeatAssignment?: string;
  analysis: CourseAutomationAnalysis;
  printResults: CoursePrintResult[];
  organizeGroups?: CourseOrganizeGroup[];
  organizeCanUndo?: boolean;
  lastOrganized?: number;
  itemStates?: Record<string, string>;
  runLog?: CourseRunLogEntry[];
  aiUsage?: unknown[];
  artifacts?: unknown[];
  activityDetailCache?: unknown[];
}

export interface CourseAutomationView {
  config: CourseAutomationConfig;
  status: CourseAutomationStatus;
}

export interface CourseAgentControlModule {
  running: boolean;
  stageLabel: string;
  progressPct: number;
  hasProgress: boolean;
  failureNote: string;
  barState: "busy" | "error" | "idle";
  text: string;
}

export interface CourseAgentHighlightModule {
  summary: string;
  immediateCount: number;
  lastChecked: string;
}

export interface CourseAgentStandingModule {
  active: CourseAgentItem[];
  archive: CourseArchivedGroup[];
}

export interface CourseAgentSeatModule {
  seat?: CourseSeatConclusion;
  confidencePct: number;
}

export interface CourseAgentPrintModule {
  results: CoursePrintResult[];
  pending: number;
  failures: number;
  unresolvedCandidates: CoursePrintCandidate[];
}

export interface CourseAgentDocumentsModule {
  items: CourseDocumentAnalysis[];
  pendingCount: number;
  sourceEvents: CourseSourceEvent[];
}

export interface CourseAgentOrganizeModule {
  groups: CourseOrganizeGroup[];
  fileCount: number;
  canUndo: boolean;
  lastOrganized?: number;
}

export interface CourseAgentLogModule {
  entries: CourseRunLogEntry[];
  failureNote: string;
}

export type CourseAgentPendingKind = "notice" | "print" | "seat";

export interface CourseAgentPendingItem {
  id: string;
  kind: CourseAgentPendingKind;
  text: string;
  detail?: string;
  expiresAt?: string;
  tone?: "info" | "warn";
  target?: Exclude<CourseAgentDetailKey, "pending">;
  sourceId?: string;
}

export interface CourseAgentPendingModule {
  items: CourseAgentPendingItem[];
}

export type CourseAgentPillIcon =
  | "list.checks"
  | "seat"
  | "note"
  | "printer"
  | "folder.open"
  | "doc.search";

export interface CourseAgentPillEntry {
  key: CourseAgentDetailKey;
  icon: CourseAgentPillIcon;
  label: string;
  badge: string | number;
  badgeClass?: string;
  title?: string;
}

export interface CourseAgentNavigationModule {
  detailCounts: Record<CourseAgentDetailKey, number | null>;
  pills: CourseAgentPillEntry[];
}

export type CourseAgentModuleSignatures = Record<CourseAgentModuleKey, string>;

export interface CourseAgentModules {
  control: CourseAgentControlModule;
  highlight: CourseAgentHighlightModule;
  pending: CourseAgentPendingModule;
  standing: CourseAgentStandingModule;
  seat: CourseAgentSeatModule;
  print: CourseAgentPrintModule;
  organize: CourseAgentOrganizeModule;
  documents: CourseAgentDocumentsModule;
  log: CourseAgentLogModule;
  navigation: CourseAgentNavigationModule;
  signatures: CourseAgentModuleSignatures;
}

export function stageText(stage: string, enabled = true): string {
  return (
    {
      checking: "更新を確認中",
      downloading: "資料を取得中",
      analyzing: "資料を一件ずつ分析中",
      summarizing: "個別摘要を最終確認中",
      pending_summary: "新しい摘要を蓄積中",
      printing: "印刷を検証中",
      unchanged: "変更なし",
      done: "確認済み",
      error: "確認エラー",
    }[stage] || (enabled ? "次の確認を待機中" : "無効")
  );
}

export function timeAgo(secs: number, nowMs = Date.now()): string {
  if (!secs) return "";
  const delta = nowMs / 1000 - secs;
  if (delta < 60) return "たった今";
  if (delta < 3600) return `${Math.floor(delta / 60)}分前`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}時間前`;
  return `${Math.floor(delta / 86400)}日前`;
}

export function kindLabel(kind: string): string {
  switch (kind) {
    case "material":
      return "教材";
    case "announcement":
      return "お知らせ";
    case "report":
      return "課題";
    default:
      return kind || "資料";
  }
}

export function printLabel(status: string): { text: string; tone: "ok" | "warn" | "bad" | "info" } {
  switch (status) {
    case "printed":
      return { text: "印刷済み", tone: "ok" };
    case "already_printed":
      return { text: "印刷済み", tone: "ok" };
    case "error":
      return { text: "印刷失敗", tone: "bad" };
    case "not_found":
      return { text: "対象不明", tone: "bad" };
    case "needs_confirmation":
      return { text: "確認待ち", tone: "info" };
    case "dispatching":
      return { text: "送信中", tone: "info" };
    case "unknown":
      return { text: "結果要確認", tone: "warn" };
    case "skipped_low_confidence":
      return { text: "確度不足で保留", tone: "warn" };
    default:
      return { text: status, tone: "warn" };
  }
}

export function buildCourseAgentModules(
  status: CourseAutomationStatus | null | undefined,
  options: { enabled?: boolean; localError?: string; now?: Date } = {},
): CourseAgentModules {
  const enabled = options.enabled ?? true;
  const now = options.now ?? new Date();
  const nowMs = now.getTime();
  const today = dateKey(now);
  const analysis = status?.analysis;
  const documentAnalyses = status?.documentAnalyses ?? [];
  const printResults = status?.printResults ?? [];
  const stageLabel = stageText(status?.stage || "", enabled);
  const hasProgress = (status?.totalDocuments ?? 0) > 0;
  const progressPct = hasProgress
    ? Math.min(100, Math.round(((status?.processedDocuments ?? 0) / (status?.totalDocuments ?? 1)) * 100))
    : 0;
  const failureNote = options.localError || status?.lastError || "";
  const running = status?.running ?? false;
  const control: CourseAgentControlModule = {
    running,
    stageLabel,
    progressPct,
    hasProgress,
    failureNote,
    barState: running ? "busy" : failureNote ? "error" : "idle",
    text: running && hasProgress ? `${progressPct}% ・ ${stageLabel}` : stageLabel,
  };

  const highlight: CourseAgentHighlightModule = {
    summary: analysis?.summary?.trim() ?? "",
    immediateCount: documentAnalyses.filter(
      (doc) => doc.status === "done" && doc.triggerDecision === "immediate",
    ).length,
    lastChecked: status?.lastRun ? timeAgo(status.lastRun, nowMs) : "未確認",
  };

  const itemStates = status?.itemStates ?? {};
  const isHandled = (id: string) => itemStates[id] === "done" || itemStates[id] === "known";
  const isExpired = (item: { expiresAt?: string }) =>
    !!item.expiresAt && item.expiresAt.slice(0, 10) < today;

  const visibleNotices = (analysis?.findings ?? [])
    .filter((finding) => !finding.flags?.action && !isExpired(finding) && !isHandled(finding.id))
    .slice()
    .sort((a, b) => Number(Boolean(b.expiresAt)) - Number(Boolean(a.expiresAt)));

  const activeStanding = (analysis?.standingContext ?? [])
    .filter((memory) => !isExpired(memory))
    .slice()
    .sort((a, b) => Number(b.flags?.urgent ?? false) - Number(a.flags?.urgent ?? false));
  const standingArchive: CourseArchivedGroup[] = [];
  const stragglers = (analysis?.standingContext ?? []).filter(isExpired).map((memory) => memory.text);
  if (stragglers.length) standingArchive.push({ label: "最近終了", items: stragglers });
  for (const group of analysis?.archivedContext ?? []) {
    if (group.items?.length) standingArchive.push(group);
  }
  const standing: CourseAgentStandingModule = {
    active: activeStanding,
    archive: standingArchive,
  };

  const seatValue = analysis?.seat;
  const seat: CourseAgentSeatModule = {
    seat: seatValue?.assignment ? seatValue : undefined,
    confidencePct: Math.round(Math.max(0, Math.min(1, seatValue?.confidence ?? 0)) * 100),
  };

  const resultFilenames = new Set(printResults.map((result) => result.filename));
  const unresolvedCandidates = (analysis?.printCandidates ?? []).filter(
    (candidate) => candidate.filename && !resultFilenames.has(candidate.filename),
  );
  const print: CourseAgentPrintModule = {
    results: printResults,
    pending: printResults.filter((result) => result.status === "needs_confirmation").length,
    failures: printResults.filter(
      (result) => result.status === "error" || result.status === "not_found" || result.status === "unknown",
    ).length,
    unresolvedCandidates,
  };

  const pendingItems: CourseAgentPendingItem[] = visibleNotices.map((finding) => ({
    id: `notice:${finding.id}`,
    kind: "notice",
    text: finding.text,
    expiresAt: finding.expiresAt,
    tone: "info",
    sourceId: finding.id,
  }));
  for (const result of printResults) {
    if (result.status === "needs_confirmation") {
      pendingItems.push({
        id: `print-confirm:${result.actionKey || result.filename}:${result.category ?? ""}`,
        kind: "print",
        text: `印刷の許可待ち: ${result.filename}`,
        detail: result.category ? `種別「${result.category}」を許可すると以降は自動印刷します。` : "この印刷だけ許可が必要です。",
        tone: "info",
        target: "print",
      });
    } else if (result.status === "error" || result.status === "not_found" || result.status === "unknown") {
      pendingItems.push({
        id: `print-check:${result.actionKey || result.filename}:${result.status}`,
        kind: "print",
        text: `印刷結果の確認: ${result.filename}`,
        detail: result.detail || printLabel(result.status).text,
        tone: "warn",
        target: "print",
      });
    }
  }
  const pendingSeatNotification = Boolean(status?.pendingSeatNotification && seat.seat?.assignment);
  if (pendingSeatNotification && seat.seat) {
    pendingItems.push({
      id: `seat:${seat.seat.assignment}`,
      kind: "seat",
      text: `座席情報が更新されています: ${seat.seat.assignment}`,
      detail: "通知に失敗した可能性があります。座席モジュールで根拠を確認してください。",
      tone: "info",
      target: "seat",
    });
  }
  const pending: CourseAgentPendingModule = {
    items: pendingItems,
  };

  const organizeGroups = (status?.organizeGroups ?? []).filter((group) => group.files?.length);
  const organize: CourseAgentOrganizeModule = {
    groups: organizeGroups,
    fileCount: organizeGroups.reduce((total, group) => total + (group.files?.length ?? 0), 0),
    canUndo: Boolean(status?.organizeCanUndo),
    lastOrganized: status?.lastOrganized,
  };

  const documents: CourseAgentDocumentsModule = {
    items: documentAnalyses.filter((doc) => doc.status === "done" && doc.summary?.trim()),
    pendingCount: status?.pendingSummaryIds?.length ?? 0,
    sourceEvents: status?.sourceEvents ?? [],
  };

  const log: CourseAgentLogModule = {
    entries: (status?.runLog ?? []).slice().reverse(),
    failureNote,
  };

  const navigation: CourseAgentNavigationModule = buildNavigation(pending, seat, standing, print, organize, documents, log);
  const signatures: CourseAgentModuleSignatures = {
    control: signatureOf({
      running: status?.running,
      stage: status?.stage,
      totalDocuments: status?.totalDocuments,
      processedDocuments: status?.processedDocuments,
      lastError: status?.lastError,
      localError: options.localError,
    }),
    highlight: signatureOf({
      summary: analysis?.summary,
      lastRun: status?.lastRun,
      triggerDecisions: documentAnalyses.map((doc) => [doc.id, doc.status, doc.triggerDecision]),
    }),
    pending: signatureOf({
      findings: analysis?.findings,
      itemStates,
      today,
      printResults: printResults.map((result) => [
        result.actionKey,
        result.filename,
        result.status,
        result.detail,
        result.category,
      ]),
      pendingSeatNotification: status?.pendingSeatNotification,
      seatAssignment: analysis?.seat?.assignment,
    }),
    standing: signatureOf({ standingContext: analysis?.standingContext, archivedContext: analysis?.archivedContext, today }),
    seat: signatureOf({ seat: analysis?.seat, pendingSeatNotification: status?.pendingSeatNotification }),
    print: signatureOf({ printCandidates: analysis?.printCandidates, printResults }),
    organize: signatureOf({
      groups: organizeGroups.map((group) => [
        group.id,
        group.label,
        group.folder,
        (group.files ?? []).map((file) => file.filename),
      ]),
      canUndo: status?.organizeCanUndo,
      lastOrganized: status?.lastOrganized,
    }),
    documents: signatureOf({
      documentAnalyses: documentAnalyses.map((doc) => [
        doc.id,
        doc.kind,
        doc.title,
        doc.filename,
        doc.status,
        doc.summary,
        doc.triggerDecision,
      ]),
      pendingSummaryIds: status?.pendingSummaryIds,
      sourceEvents: status?.sourceEvents,
    }),
    log: signatureOf({ runLog: status?.runLog, lastError: status?.lastError, localError: options.localError }),
    navigation: "",
  };
  signatures.navigation = signatureOf({
    pending: signatures.pending,
    seat: signatures.seat,
    standing: signatures.standing,
    print: signatures.print,
    organize: signatures.organize,
    documents: signatures.documents,
    log: signatures.log,
  });

  return { control, highlight, pending, standing, seat, print, organize, documents, log, navigation, signatures };
}

export function changedCourseAgentModuleKeys(
  previous: CourseAgentModules | null | undefined,
  next: CourseAgentModules,
): CourseAgentModuleKey[] {
  if (!previous) return [...COURSE_AGENT_MODULE_KEYS];
  return COURSE_AGENT_MODULE_KEYS.filter((key) => previous.signatures[key] !== next.signatures[key]);
}

function buildNavigation(
  pending: CourseAgentPendingModule,
  seat: CourseAgentSeatModule,
  standing: CourseAgentStandingModule,
  print: CourseAgentPrintModule,
  organize: CourseAgentOrganizeModule,
  documents: CourseAgentDocumentsModule,
  log: CourseAgentLogModule,
): CourseAgentNavigationModule {
  const pills: CourseAgentPillEntry[] = [];
  if (pending.items.length) {
    const hasWarning = pending.items.some((item) => item.tone === "warn");
    pills.push({
      key: "pending",
      icon: "list.checks",
      label: "保留中",
      badge: pending.items.length,
      badgeClass: hasWarning ? "sa-pill-warn" : "sa-pill-accent",
      title: "自動検知があなたの確認を待っています",
    });
  }
  if (seat.seat?.assignment) {
    pills.push({
      key: "seat",
      icon: "seat",
      label: "座席",
      badge: seat.seat.assignment,
      badgeClass: "sa-pill-val",
      title: seat.seat.assignment,
    });
  }
  if (standing.active.length) {
    pills.push({ key: "standing", icon: "note", label: "記憶", badge: standing.active.length });
  }
  if (print.results.length || print.unresolvedCandidates.length) {
    if (print.pending > 0) {
      pills.push({
        key: "print",
        icon: "printer",
        label: "印刷",
        badge: `${print.pending} 件確認`,
        badgeClass: "sa-pill-accent",
        title: `${print.pending} 件が印刷の許可待ちです`,
      });
    } else if (print.failures > 0) {
      pills.push({
        key: "print",
        icon: "printer",
        label: "印刷",
        badge: `${print.failures} 件失敗`,
        badgeClass: "sa-pill-warn",
        title: `${print.failures} 件失敗・次回再試行します`,
      });
    } else if (print.results.length) {
      pills.push({ key: "print", icon: "printer", label: "印刷", badge: print.results.length });
    } else {
      pills.push({
        key: "print",
        icon: "printer",
        label: "印刷",
        badge: `${print.unresolvedCandidates.length} 候補`,
        badgeClass: "sa-pill-accent",
        title: "印刷候補を確認します",
      });
    }
  }
  // Always present so the entry is discoverable even before the first filing.
  pills.push({
    key: "organize",
    icon: "folder.open",
    label: "整理",
    badge: organize.groups.length || "—",
    title: organize.groups.length
      ? "資料をテーマ別に自動整理しました"
      : "資料がたまるとテーマ別に自動整理します",
  });
  if (documents.items.length || documents.pendingCount) {
    pills.push({
      key: "documents",
      icon: "doc.search",
      label: "分析",
      badge: documents.pendingCount ? `${documents.pendingCount} 件要約待ち` : documents.items.length,
      badgeClass: documents.pendingCount ? "sa-pill-accent" : undefined,
    });
  }
  return {
    pills,
    detailCounts: {
      pending: pending.items.length,
      standing: standing.active.length,
      seat: null,
      print: print.results.length + print.unresolvedCandidates.length,
      organize: organize.fileCount,
      documents: documents.items.length,
      log: log.entries.length,
    },
  };
}

function dateKey(date: Date): string {
  const month = `${date.getMonth() + 1}`.padStart(2, "0");
  const day = `${date.getDate()}`.padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

function signatureOf(value: unknown): string {
  return JSON.stringify(value ?? null);
}
