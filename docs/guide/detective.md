# Detective

Detective is KWIC's course-review game system. It turns existing Live notes and exam-related notifications into playable investigation cases.

## Product Direction

Detective is not a flashcard or vocabulary-drill surface. The player is not asked to "review cards"; they investigate a course case.

The core loop is:

1. Gather evidence from Live notes and course notifications.
2. Form a hypothesis about concepts, emphasis, or exam scope.
3. Connect concepts and source excerpts on a case board.
4. Challenge weak or incorrect explanations.
5. Close the case with a short final deduction.

The learning mechanism is embedded in the game action itself. Players learn by selecting evidence, connecting ideas, rejecting misleading claims, and explaining course material with sources.

## Naming

All user-facing names should use `Detective`.

Avoid alternative product names such as "Review Quest", "Exam Lab", or "Study Cards" in the app UI. Internally, implementation names may use `detective` for routes, components, and files.

## Design Principles

- Evidence first: every generated case item must trace back to a Live note or notification source.
- Course-bound: cases are always tied to a concrete course when course context exists.
- Healthy engagement: use curiosity, short sessions, adaptive challenge, and visible case closure.
- No gambling-like mechanics: do not use loot boxes, randomized rare rewards, global pressure leaderboards, loss-based streaks, or FOMO.
- Failure becomes a clue: incorrect or uncertain answers become unresolved doubts for future cases.
- Short playable sessions: first-version cases should fit into 8-15 minutes.

## Research-Informed Rationale

Detective uses game engagement mechanisms that are compatible with learning:

- Flow: challenge should rise with the learner's skill and current course understanding.
- Intrinsic integration: course understanding is the core mechanic, not a quiz pasted onto a game shell.
- Structural engagement: narrative, control, identity, and feedback can sustain play when they serve the learning task.
- Retrieval and explanation: the final deduction requires recalling and reorganizing source material.

Detective intentionally avoids variable-ratio rewards and loot-box-like surprise systems because those mechanisms are close to gambling-style reinforcement and are not appropriate for an academic tool.

## First Version Scope

The first implementation should establish a playable vertical slice:

1. Add a new left-menu page named `Detective`.
2. List courses that have Live notes, exam-like notifications, or schedule context.
3. Show each course's evidence inventory:
   - Live note count
   - exam signal count
   - unresolved doubt count
   - latest evidence time
4. Show a `Review Queue` for urgent doubts, low-confidence case results, and source gaps.
5. Open a course case board.
6. Provide three playable stages:
   - Evidence Hunt: choose useful source excerpts.
   - Link Board: connect concepts and evidence.
   - Final Deduction: write a short answer using the selected evidence.
7. Show `Case Strength` feedback based on evidence coverage, source fit, relationship clarity, deduction quality, and confidence.
8. Close the case and archive the deduction locally.
9. Save unresolved doubts locally when the player marks a concept as unclear.

Detective first builds an evidence package from cached Live and notification sources. When AI is enabled in Selah settings, the configured provider becomes the case director: it chooses the case type, title, difficulty, evidence-card order, card summaries, claim, final question, warnings, and stage prompts while reusing only existing evidence ids. If AI is disabled or fails, Detective keeps a deterministic fallback case.

## Data Sources

Detective should reuse existing KWIC data paths:

- Live notes:
  - `list_downloads`
  - `scan_download_dir`
  - records where `source == "live"` or filename contains `_live`
- Course and schedule context:
  - `schedule_data`
- Notifications:
  - KGC `notifications`
  - Luna `luna_updates`
  - KWIC `kwic_home`

Exam-signal keywords for the first pass:

- `試験`
- `テスト`
- `小テスト`
- `中間`
- `期末`
- `レポート試験`
- `範囲`
- `持ち込み`
- `exam`
- `quiz`
- `midterm`
- `final`

## Case Types

### Exam Signal Case

Use when a course has exam-like notifications. The goal is to identify what the notification implies and which Live notes support preparation.

### Concept Web Case

Use when a course has Live notes but little exam notification context. The goal is to connect major concepts from recent notes.

### Contradiction Case

Use when Detective can generate or infer a questionable statement. The player identifies why the statement is weak or unsupported.

### Missing Link Case

Use when two concepts appear in the same Live evidence set. The player explains the missing relationship.

### Final Deduction Case

Use as the session closer. The player writes a compact answer using at least two evidence items.

## Backend Shape

The first backend module lives in `src-tauri/src/detective.rs` and exposes:

- `detective_get_context()`
- `detective_generate_case(courseKey: String)`
- `detective_save_doubts(doubts: DetectiveDoubt[])`
- `detective_save_case_result(result: DetectiveCaseResult)`

`detective_get_context()` centralizes the first evidence pipeline:

- scans Live download records
- compacts Live markdown before the full transcript section
- loads exam-like signals from KGC, Luna, KWIC, and schedule AI cache
- groups evidence by normalized course names
- attaches unresolved doubts from the local data cache
- attaches recent closed cases from the local case archive
- builds a `Review Queue` from due doubts, low-confidence case results, and exam-signal source gaps

`detective_generate_case()` builds a playable case for a selected course:

- prepares a source-bound evidence package from Live notes, exam signals, and unresolved doubts
- asks AI to choose `Exam Signal Case`, `Concept Web Case`, `Doubt Repair Case`, `Contradiction Case`, or `Missing Link Case`
- lets AI generate the case title, difficulty, briefing, evidence order, evidence summaries, claim, final question, warnings, and three-stage Detective loop
- preserves backend-verified source-opening metadata for Live markdown, Luna detail pages, KGC detail paths, and KWIC notification details
- rejects AI evidence ids that do not exist in the prepared package
- adds source warnings when exam signals or Live notes are missing

`detective_save_case_result()` stores closed cases locally. The first result payload includes:

- selected evidence ids
- the Link Board relationship
- the final deduction
- player confidence
- close timestamp

Future AI work can extend the same evidence contract with richer distractors and adaptive scoring, but generated cases must still stay source-bound.

Generated case JSON should contain:

```ts
interface DetectiveCase {
  id: string;
  courseKey: string;
  courseName: string;
  title: string;
  caseType: "Exam Signal Case" | "Concept Web Case" | "Doubt Repair Case" | "Contradiction Case" | "Missing Link Case";
  difficulty: number;
  briefing: string;
  evidence: DetectiveEvidence[];
  stages: DetectiveStage[];
  contradictionClaim: string;
  finalQuestion: string;
  sourceWarnings: string[];
}

interface DetectiveEvidence {
  id: string;
  sourceId?: string;
  sourceType: "live" | "signal" | "doubt";
  source?: "live" | "kgc" | "luna" | "kwic" | "schedule" | "doubt";
  title: string;
  date?: string;
  excerpt: string;
  sourcePath?: string;
  sourceUrl?: string;
  informationType?: string;
  personCategoryCd?: string;
  categoryCd?: string;
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
  confidence: 1 | 2 | 3 | 4 | 5;
}

interface DetectiveReviewItem {
  id: string;
  courseKey: string;
  courseName: string;
  reason: string;
  priority: number;
  dueAt: number;
}
```

No case should assert exam scope without evidence.

## UX Notes

The page should feel like a calm investigation board rather than a mobile game:

- evidence cards
- review queue
- case strength meter
- Link Board relation lines between the central claim and selected evidence nodes
- case status
- unresolved doubt list
- short final report area

Keep the design consistent with KWIC's productivity-tool shell. Detective can feel playful, but it should not become visually noisy or pressure-heavy.
