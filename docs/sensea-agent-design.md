# SenseA Agent Design

SenseA is a low-interruption proactive assistant, not a periodic course
summary generator. Its job is to notice relevant changes, decide whether they
matter now, perform narrowly safe actions, and remember the result.

## Product Goal

SenseA should reduce work the student would otherwise need to remember or repeat:

- inspect each new or changed Luna item once;
- preserve long-lived rules and unresolved questions across runs;
- notify only when a change needs prompt attention;
- automatically print only when the source explicitly requires paper;
- infer seat changes from old context plus new evidence;
- retry failed retrieval, analysis, notification, and printing;
- avoid replaying complete history to the model.

Proactivity is useful only when timing, relevance, and action boundaries are
reliable. More notifications or more model calls are not inherently better.

## Agent Loop

```text
observe -> index one changed item -> decide impact
        -> fold deltas into working memory when needed
        -> execute safe action -> persist outcome -> observe again
```

The decision classes are:

- `immediate`: update working memory now and notify the student;
- `observe`: retain the unresolved context for comparison with later evidence;
- `routine`: retain the item analysis without interrupting the student.

This is an impact decision made by AI, not a keyword match. Deterministic code
still owns action permissions, idempotency, retries, and verification.

## Memory Tiers

| Tier | Stored data | Purpose | Sent to AI |
|---|---|---|---|
| Audit memory | artifacts and per-document analyses | provenance, reuse, retry, history | no |
| Working memory | summary, findings, standing context, seat, print candidates | current decision state | yes, compact |
| Episodic delta | newly analyzed items and source lifecycle events | update working memory | yes, once |
| Action ledger | notified versions and print results | idempotency and feedback | only when decision-relevant |

Working memory is rewritten, not appended. Expired, completed, superseded, and
duplicate items must disappear. Audit memory is never used as a prompt dump.
When working memory grows, the prompt layer sends a bounded projection:
summary, current seat, print candidates, urgent/action items, and as much
archived context as fits. The full archive remains persisted outside the prompt
and is merged back during normalization, so omitted history is not erased.

## Token Budget Rules

1. Individual analysis receives one document and a whitelisted student profile.
2. Individual output is a compact index, not prose suitable for display.
3. Comprehensive analysis receives only a bounded prompt projection of working
   memory plus new deltas.
4. Large delta sets are folded recursively in bounded batches.
5. Model output is normalized before persistence, so verbose violations are not
   replayed forever.
6. Request and response sizes are logged and the latest bounded estimates are
   persisted on the course status for timeout and limit debugging.
7. Retries reuse the same compact request and are limited to failures that are
   unlikely to have completed a generation. Response-body interruptions and
   SenseA-level timeouts are not automatically replayed, because they may
   already have consumed provider tokens.

Normal checks are delta-first. A cycle indexes the current Luna source list, but
only new, changed, retryable, or explicitly selected documents enter the AI
document loop. Same-source successful downloads remain in the artifact ledger
for printing and provenance, but are not re-materialized as analysis documents
unless a legacy identity needs one migration pass. A full sweep requires the
explicit re-analyze-all command.
Activity details and attachments are discovered from the current course page,
not from stale activity rows in the local database. The database can preserve
evidence and artifact paths, but it must not keep removed activities alive as
fresh input.
Unchanged activity detail pages can reuse a short-lived detail cache so
deferred follow-up cycles do not re-fetch every announcement or assignment. The
cache is keyed by the current course-list fingerprint and expires on the next
normal check window; forced re-analysis bypasses it. If revalidation fails but a
matching stale detail cache exists, the stale cache can preserve source
continuity without extending its lifetime. A transient fetch error must not
create a synthetic source change. Retryable activity attachment failures bypass
the detail cache so the attachment list is refreshed and the failed download can
retry immediately.
When multiple pending deltas are folded, `immediate` items are sent before
`observe` and routine backlog so urgent updates cannot be trapped behind older
low-impact summaries.
When a normal cycle finds a newly analyzed `immediate` document, it may pause the
remaining delta work, fold the urgent item into working memory, notify, and let
the next cycle continue the leftover documents. That continuation is queued
shortly after the urgent run finishes, rather than waiting for the course's
normal interval. Automatic scheduled and deferred cycles re-check the course's
enabled state at execution time, so disabling SenseA stops queued background
work before it starts. This check happens after the cycle acquires exclusive
status-write access, closing the window where a queued job was enabled when
enqueued but disabled while waiting behind another cycle. Scheduled cycles also
re-check `lastRun` at that same point, so stale queued schedule jobs collapse
after another cycle has already refreshed the course. Deferred follow-ups bypass
the normal interval but still obey the enabled-state check.

The current estimator is deliberately conservative for mixed Japanese and
English text. Provider-side token usage should replace it when uniformly
available.

## Safe Action Boundary

SenseA may act automatically only where the action is narrow and verifiable:

- native notification for an AI-classified immediate item;
- native notification for a changed seat conclusion;
- printing when the source explicitly requests printing/paper/handwriting and
  the candidate passes the confidence threshold.

Every action uses a persistent identity. Successful actions are not repeated;
failed actions remain pending and retry on a later run. Print actions also write
a dispatch ledger before calling the OS printer. If the process loses the final
receipt after dispatch, the action becomes `unknown` and is not automatically
replayed, because duplicate paper output is worse than a visible confirmation
request. Arbitrary external actions require a future permission and confirmation
design.

## Failure Invariants

- A successful download or analysis is reused and never replaced by a later
  transient failure.
- A fingerprinted download is reused only when the current source fingerprint
  matches. Filename-only reuse is reserved for legacy records that do not yet
  have a fingerprint, and only to migrate them forward.
- A changed document version can trigger a new notification.
- A source removal or replacement is detected from the current Luna source list,
  not from transient download or detail-fetch failures. The event is kept as a
  pending delta until a comprehensive summary consumes it. When the source list
  exposes a new source fingerprint, replacement can be recorded even if the new
  file body was not successfully downloaded in that cycle.
- A failed notification or print remains retryable.
- A dispatched print whose final receipt is unknown remains visible but is not
  treated as retryable.
- A process-level failure cannot leave the global run lock permanently set.
- A comprehensive summary checkpoints each successful batch: consumed pending
  IDs are removed and the updated working memory is saved before the next batch.
  If a later batch fails, the next run retries only the remaining deltas.
- A single `observe` document can wait, but an unresolved observed item plus
  later pending evidence triggers a comprehensive re-evaluation.
- Time passing is deterministic: expired working-memory items are pruned or
  archived on a normal check even when no AI summary runs.
- Generic `data_cache` is durable app/user state and must survive schema
  migrations of schedule-derived tables.
- No AI response may directly bypass deterministic action checks.

## Scheduling and Concurrency

SenseA is driven by a single background loop, not a per-course timer. The loop
starts after a short delay so application startup is never blocked, then wakes
on a fixed cadence and asks which courses are due. A course is due when its
configured interval has elapsed since its last run; the interval has a lower
floor so misconfiguration cannot turn SenseA into a busy loop. A manual run is
allowed only for an already-enabled course and shares the same path as a
scheduled run.

One process-wide run lock serializes course runs, so two courses never analyze,
notify, or print concurrently. The lock is released by a drop guard, which
clears it even when a run returns early or panics. This is what makes the lock
invariant — "a process-level failure cannot leave the global run lock
permanently set" — hold in practice rather than only by convention.

## Configuration Surface

Every decision boundary the student can move lives in per-course configuration,
not in the model prompt:

- `enabled`: SenseA ignores the course entirely until this is set.
- `interval_minutes`: how often the course becomes due.
- `monitor_materials`, `monitor_announcements`, `monitor_assignments`: which
  Luna sources are observed.
- `analyze_all`: analyze every item versus only what changed.
- `auto_print`: permit the printing action at all.
- `notify_seat_changes`: permit the seat-change notification at all.

The model classifies impact; configuration decides whether a class of action is
permitted. An action the student has disabled is never taken regardless of what
the model concludes, which keeps the impact decision and the action permission
strictly separated.

## Dock Module Contract

The 自動検知/SenseA dock is a set of independently derived modules, not one blended
summary card. `courseAgentModules.ts` is the contract layer between persisted
course status and Svelte rendering. Each module owns the smallest display model
it needs and a signature of the fields that can change it; the dock only assigns
a module when that signature changes.

| Module | Displays | Updates from |
|---|---|---|
| `control` | current stage, progress, error tone, manual refresh | `running`, `stage`, progress counters, local/backend error |
| `highlight` | current overall summary and last checked time | `analysis.summary`, `lastRun`, immediate document decisions |
| `pending` | non-TODO notices and shortcuts to blocked module actions | `analysis.findings`, `itemStates`, `printResults`, seat notification state, current date |
| `standing` | active long-lived memory plus expired/reference archive | `analysis.standingContext`, `analysis.archivedContext`, current date |
| `seat` | seat assignment, confidence, evidence | `analysis.seat`, seat notification state |
| `print` | print results, approval waits, unresolved candidates | `analysis.printCandidates`, `printResults` |
| `documents` | per-document summaries and pending final-summary count | `documentAnalyses`, `pendingSummaryIds`, `sourceEvents` |
| `log` | recent run log and current error detail | `runLog`, local/backend error |
| `navigation` | visible drill-in pills and detail counts | the module signatures above |

This gives the UI atomic render boundaries today. The backend still emits a full
`CourseAutomationStatus` snapshot for compatibility; a future payload-saving
step can emit module patches keyed by the same signatures without redesigning
the dock.

`analysis.findings` remains the backend working-memory field, but the dock does
not present it as a general "confirmations" list. Action findings are reconciled
to TODO and intentionally hidden from `pending`; non-action findings appear only
when they are short-lived notices the student must explicitly know. Print
approval/failure and seat-notification retry states enter `pending` as shortcuts
to their owner modules rather than duplicating those modules' full content.

## Platform Support

Both automatic actions are implemented so that success is observable:

- Notifications use the native OS facility on every platform. A failed
  notification leaves its pending identity set and retries on the next run.
- Printing first confirms a default printer exists, then submits the file, and
  treats the submission receipt as proof. On macOS and Linux this is `lpstat -d`
  followed by `lp`, verified by the returned request id. On Windows it is a
  default-printer query followed by the shell print verb, verified by the
  printer name in the receipt. With no default printer the action fails and
  stays retryable rather than silently succeeding.

Printing confirms that the job was accepted by the spooler, not that paper left
the tray; completion tracking is out of scope for the safe-action boundary.

## Open Questions

- The token estimator is conservative for mixed Japanese and English text and
  should be replaced by provider-reported usage once it is uniformly available.
- Windows print verification confirms submission, not spooler completion;
  richer confirmation would need per-job polling.
- The print and seat confidence thresholds are fixed and not yet adjusted from
  action feedback.
- Actions beyond notification and printing require the future permission and
  confirmation design referenced in the safe-action boundary.

## Research Basis

- [ReAct](https://arxiv.org/abs/2210.03629): reasoning and actions should form
  an interleaved loop; actions supply new observations for later decisions.
- [Reflexion](https://arxiv.org/abs/2303.11366): action feedback should be kept
  as episodic memory so later attempts improve instead of repeating failure.
- [Generative Agents](https://arxiv.org/abs/2304.03442): observation, planning,
  and reflection are all necessary; raw experience should be synthesized into
  higher-level memory.
- [MemGPT](https://arxiv.org/abs/2310.08560): hierarchical memory is preferable
  to treating the context window as the entire memory system.
- [Lost in the Middle](https://arxiv.org/abs/2307.03172): simply adding more
  context can make relevant evidence harder to use.
- [LongLLMLingua](https://arxiv.org/abs/2310.06839): compact prompts reduce cost
  and can improve long-context reasoning.
- [Summ^N](https://arxiv.org/abs/2110.10150): recursive, bounded summarization
  supports inputs larger than a single model context.
- [MemoryBank](https://arxiv.org/abs/2305.10250): long-term memory should be
  selectively updated and reinforced rather than replayed wholesale.
- [Need Help? Designing Proactive AI Assistants for Programming](https://arxiv.org/abs/2410.04596):
  proactive assistance is a mixed-initiative interaction problem whose timing
  and integration determine usefulness.

## Review Checklist

For every SenseA change:

1. Does it improve observe-decide-act-feedback behavior?
2. Can it create duplicate calls, notifications, downloads, or prints?
3. Is complete history being sent where a compact delta would work?
4. Does every failure retain enough state to retry?
5. Can the action be verified and safely repeated?
6. Are new state fields backward-compatible through serde defaults?
7. Do tests cover changed versions, failure recovery, and bounded context?
