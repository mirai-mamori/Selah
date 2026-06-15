//! Every AI prompt builder (outline / draft / editor / bible / finale) + section renderers.
use super::*;

pub(crate) fn finale_system_prompt() -> &'static str {
    "You are the lead writer closing out a long-running 逆転裁判-style study-mystery campaign. The season is complete. Write the GRAND FINALE — the epilogue shown once, when the player has cleared every chapter.\n\nThis finale must PAY OFF what actually happened, not restate the premise: resolve the 暗线 conspiracy conclusively (name the antagonist force + their motive), land the setups that were planted across the staged reveals, honour the established canon facts, and give the recurring cast a final beat consistent with their motivation/stakes. End on a strong closing image. 4–6 Japanese sentences, evocative and conclusive — no cliffhanger.\n\nALSO refine each staged reveal so the four-stage 暗线 arc reads as one coherent build toward this finale, consistent with the accumulated canon (keep each reveal 1–2 Japanese sentences; do not change the number of stages).\n\nOUTPUT — a single JSON object, nothing else: {\"finale\": \"…4–6文の日本語…\", \"reveals\": [{\"stage\": 1, \"reveal\": \"…\"}, {\"stage\": 2, \"reveal\": \"…\"}, {\"stage\": 3, \"reveal\": \"…\"}, {\"stage\": 4, \"reveal\": \"…\"}]}"
}

pub(crate) fn finale_user_prompt(c: &DetectiveCampaign) -> String {
    let cast = if c.cast.is_empty() {
        "(なし)".to_string()
    } else {
        c.cast
            .iter()
            .map(|m| {
                format!(
                    "- {}（{}）動機:{} 利害:{}",
                    m.name, m.role, m.motivation, m.stake
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let arc = if c.meta_arc.is_empty() {
        "(なし)".to_string()
    } else {
        c.meta_arc
            .iter()
            .map(|r| format!("- 第{}段階「{}」: {}", r.stage, r.title, r.reveal))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let facts = if c.canon.facts.is_empty() {
        "(なし)".to_string()
    } else {
        c.canon
            .facts
            .iter()
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let chapters = if c.chapters.is_empty() {
        "(なし)".to_string()
    } else {
        c.chapters
            .iter()
            .map(|ch| format!("- {}", ch.title))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        r#"世界観: {label}
舞台設定: {setting}
暗线（meta-mystery）: {meta}

登場人物:
{cast}

段階的に明かされた真相（metaArc — すべて解禁済み）:
{arc}

積み上がった正典（実際に起きた事実）:
{facts}

辿ってきた章:
{chapters}

═══ TASK ═══
上のすべてを踏まえ、キャンペーンの大団円（finale）を書いてください。暗线を決定的に解決し、各段階の布石を回収し、正典と矛盾せず、登場人物に最後の一拍を与え、強い締めの画で終える。さらに、4段階の reveal を実際に積み上がった正典と一貫するように書き直す（段階数は変えない）。JSON のみで返す: {{"finale": "…", "reveals": [{{"stage": 1, "reveal": "…"}}, {{"stage": 2, "reveal": "…"}}, {{"stage": 3, "reveal": "…"}}, {{"stage": 4, "reveal": "…"}}]}}"#,
        label = c.world_label,
        setting = c.setting,
        meta = c.meta_mystery,
        cast = cast,
        arc = arc,
        facts = facts,
        chapters = chapters,
    )
}


pub(crate) fn detective_ai_system_prompt() -> &'static str {
    "You author a Japanese Ace-Attorney-style cross-examination case that helps the player STUDY THE TESTABLE LECTURE CONTENT from the supplied Live notes. The case is review study — not a slice-of-life game.\n\nOUTPUT FORMAT — MANDATORY:\n- Output a single JSON object and nothing else.\n- Start your response with `{` and end with `}`.\n- Do NOT wrap in markdown code fences (```).\n- Do NOT add prose, preamble, explanation, comments, or trailing text.\n- Do NOT use a tool call wrapper; just emit raw JSON.\n\nINPUT KINDS:\n- LIVE notes (sourceType=live): the body text is what the teacher actually taught. THIS IS THE PRIMARY SOURCE. Extract testable knowledge from here: concepts, definitions, taxonomies, examples, formulas, theorems, classifications, historical facts, named processes, instructor explanations of meaning.\n- SIGNAL notifications (sourceType=signal): use ONLY for exam-CONTEXT (exam date, exam range, format, allowed materials, number of questions, weighting). Use them to anchor the urgency, NEVER as a topic.\n- DOUBT items (sourceType=doubt): pull the player's previously-flagged knowledge gaps as priority topics.\n\nABSOLUTE FOCUS RULE — every evidence card body, every testimony statement, every press response must be about TESTABLE COURSE KNOWLEDGE. The following CONTENT IS FORBIDDEN regardless of whether it appears in the supplied notes:\n- Administrative trivia: 学籍番号 / ネームカード / レポート提出方法 / 用紙の色・サイズ / 出欠の取り方 / 教室の場所 / 持参物 (pen, USB) / 提出期限の机械的記述 / ファイル命名規則 / Word vs PDF / その他「事務連絡」\n- Class logistics: 休講連絡, 補講日程, 教員の余談, アイスブレイクの内容, 自己紹介, 出席確認のやり方\n- General-knowledge questions that don't tie back to a specific concept the teacher explained\n- Filenames (.md, _live), ISO dates (YYYY-MM-DD), course codes, instructor names, classroom numbers\n- Generic placeholder labels (ライブメモ, 授業ノート, 講義メモ, 本講義の記録)\n- Empty platitudes (重要な内容がある, 記録が残っている, 資料を確認できる)\n- Any fact, number, date, or chapter not literally present in the supplied content\n\nIf the supplied Live notes are MOSTLY administrative and contain little testable content, prefer producing FEWER evidence cards and FEWER lies — quality over quantity. Never invent topics to fill a quota.\n\nCHAPTER SCOPE — a chapter is the WHOLE lecture turned into a play. AIM HIGH: pull as many distinct testable points from the supplied Live note as it supports — definitions, examples, contrasts, numerical claims, classifications, named processes, instructor explanations. A thin chapter (few cards, few statements, single-line teaching) is a FAILURE; depth and breadth are mandatory.\n\nCHAPTER STRUCTURE — you write ONE chapter told in 6–8 ACTS (幕), like a 逆転裁判 episode. Acts ALTERNATE between two kinds:\n- INVESTIGATION act (kind=\"investigation\"): the teaching beat. A `narrative` (2–4 Japanese sentences) advances the plot, and 2–4 `evidence` cards reveal distilled facts from the content. The first act MUST be investigation.\n- TESTIMONY act (kind=\"testimony\"): the testing beat. A `narrative` brings a witness to the stand, and 3–5 `testimony` statements follow with EXACTLY ONE lie (`isFalse: true`). The TRUE statements are NOT filler — each is its own testable concept the player should learn.\n\nKEY CONSTRAINT — teaching before testing: a lie's `keyEvidenceId` MUST point at an evidence card revealed in an EARLIER investigation act. Never test a fact the player has not yet been shown. Provide at least 3 investigation acts and at least 3 testimony acts.\n\n- `scenario` (4–6 Japanese sentences): chapter prologue / hook — situation, witnesses, stakes — anchored in real concepts from the Live notes.\n- Per testimony act: `witnessName` (2–6 Japanese chars, an invented given name e.g. ミナミ/ジュン/ハル — NEVER an instructor name) and `witnessRole` (4–14 chars). Vary witnesses across testimony acts when natural.\n- All testimony in plain Japanese witness speech (〜だ / 〜である / 〜のはず), each statement under 140 characters. The true statements must reference DIFFERENT testable points (not paraphrases of each other).\n- Each evidence `body` is 2–5 sentences: state the fact, then add ONE concrete grounding element (example / counter-example / value / contrast / named instance the teacher used).\n\nFor every testimony statement, also provide:\n- `highlights`: 1–3 keywords COPIED VERBATIM from `text` — the concept name, value, or term to scrutinise.\n- `pressResponse`: 2–3 Japanese sentences. For TRUE statements, USE THIS TO TEACH — start from the concept and drill deeper (definition → concrete example → contrast / common confusion). For the FALSE statement, the witness doubles down for 2–3 sentences (deflect, change the subject, cite an unrelated 'fact'), but never reveals the lie.\n\nNARRATIVE & MOTIVATION: each act's `narrative` is a beat of the chapter's main plot, set in the campaign world's specific historical locus (era / named place / community). **Every character who speaks or acts must have a stated or inferable motive — what they want, what they protect**. The bible's cast comes with 背景/動機/利害; honour those. Witnesses you newly invent for this chapter need a one-sentence backstory + a reason for being on the stand, established in the testimony act's `narrative` before they speak. A witness whose lie has no plausible motive (cover an ally, save reputation, conceal involvement, defend a payoff) is a failure — make the motive shape their tone in `pressResponse`, without ever stating 「私は嘘をついている」. Seed the overarching hidden thread (暗线) with ONE subtle hint in a single early act (mark that act `seedsMeta: true`). Story is the vehicle that makes the testable content stick — never invent testable facts to serve story, and never invent story so thin that characters feel like quiz props.\n\nPROFESSIONAL SCREENWRITING BAR: write at the level of a produced 逆転裁判 scenario. Scenes open in the middle of tension (in medias res), each act ends on a hook that pulls into the next, dialogue has subtext and voice (witnesses don't narrate exposition — they reveal it under pressure), and the chapter has a clear dramatic shape (掴み → 転 → 山場 → 解決). The 明线 (this chapter's case) must be self-contained and fully resolved here; the 暗线 (the season's hidden conspiracy) advances by exactly the ONE planted beat the outline specifies — no more, no less.\n\nYOU ARE GIVEN AN APPROVED OUTLINE (推理プロット): follow its act plan, its planted lie per testimony act (`lieAbout`), its coverage plan, and its caseLogic. Do not invent a different culprit, motive, or structure. Realise the outline as polished prose.\n\nALSO EMIT (top-level): `caseLogic` { truth, culprit (prefer a bible cast name), motive, redHerrings[], deductionChain[] — the ordered steps by which the busted contradictions reconstruct the truth, the last step answering finalQuestion } and `metaBeat` (one Japanese sentence: what this chapter contributed to the 暗线, matching the outline's planted beat). These must be CONSISTENT with the written acts."
}

/// Build the AI user prompt. AI reads the raw source content and distills
/// it into N short evidence cards (one-paragraph facts), each tagged with a
/// `sourceRef` pointing back to an input alias (l1/s1/d1) so we can attach
/// the original file path or URL afterwards.
pub(crate) fn detective_ai_user_prompt(
    case: &DetectiveCase,
    input: &[EvidenceInputEntry],
    memory: &DetectiveMemory,
    campaign: Option<&DetectiveCampaign>,
    syllabus: &[PlannedSession],
    knowledge: &[KnowledgePoint],
    targets: &GenTargets,
    plan: &ChapterPlan,
) -> String {
    fn render(entry: &EvidenceInputEntry, max_chars: Option<usize>) -> String {
        let body = entry.raw_content.replace('\r', "");
        let body = body.trim();
        let content = match max_chars {
            Some(cap) => truncate_chars(body, cap),
            None => body.to_string(),
        };
        let title_line = if entry.source_type == "signal" && !entry.raw_title.trim().is_empty() {
            format!("\n  title: {}", truncate_chars(&entry.raw_title, 120))
        } else {
            String::new()
        };
        format!(
            "- id: {alias}{title_line}\n  content:\n    {content}",
            alias = entry.alias,
            content = content.replace('\n', "\n    ")
        )
    }

    let live_section = collect_section(input, "live", None);
    let signal_section = collect_section(input, "signal", Some(400));
    let doubt_section = collect_section(input, "doubt", Some(280));

    let syllabus_section = if syllabus.is_empty() {
        "(この科目の授業計画は未取得。sessionNum は 0 とすること)".to_string()
    } else {
        syllabus
            .iter()
            .map(|s| {
                let mode = if s.online {
                    "（オンライン）"
                } else {
                    ""
                };
                format!(
                    "- 第{}回{}: {}",
                    s.num,
                    mode,
                    truncate_chars(s.topic.trim(), 90)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let knowledge_section = if knowledge.is_empty() {
        "(知識点リスト未取得)".to_string()
    } else {
        knowledge
            .iter()
            .map(|p| {
                let star = if p.must_cover { "★" } else { " " };
                format!(
                    "- {star} `{}` {} — {}",
                    p.id,
                    p.label,
                    truncate_chars(p.gist.trim(), 90)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let must_cover_ids: Vec<&str> = knowledge
        .iter()
        .filter(|p| p.must_cover)
        .map(|p| p.id.as_str())
        .collect();
    let must_cover_list = if must_cover_ids.is_empty() {
        "(なし)".to_string()
    } else {
        must_cover_ids.join(", ")
    };

    fn collect_section(
        input: &[EvidenceInputEntry],
        source_type: &str,
        max_chars: Option<usize>,
    ) -> String {
        let blocks: Vec<String> = input
            .iter()
            .filter(|e| e.source_type == source_type)
            .map(|e| render(e, max_chars))
            .collect();
        if blocks.is_empty() {
            "(none)".to_string()
        } else {
            blocks.join("\n")
        }
    }

    format!(
        r#"Course: {course}

═══ INPUT SOURCES — read these in full and extract concrete facts ═══

Live lecture notes (PRIMARY — body is what the teacher actually said):
{live}

Notifications (exam format / scope intel):
{signals}

Student's unresolved doubts:
{doubts}

═══ 授業計画 (lecture plan — match THIS lecture to its 第N回) ═══
{syllabus}

═══ KNOWLEDGE POINTS (★ = must-cover; you MUST place each ★ point somewhere in the chapter, and at least {coverage_min} total) ═══
{knowledge}
must-cover ids: {must_cover_list}

═══ CAMPAIGN WORLD (世界観 — this session is a chapter inside it) ═══
{campaign}

═══ MEMORY (player history — drive continuity) ═══
{memory}

═══ TASK ═══
Write ONE CHAPTER of the campaign — a substantial case told in {acts_min}–{acts_max} ACTS (幕). This is the WHOLE lecture turned into a play; aim for breadth + depth, NOT a quick quiz. The chapter plays like a 逆転裁判 episode: the detective ALTERNATES between INVESTIGATION acts (teach concrete facts) and TESTIMONY acts (catch a witness in a lie). Cover every NOTABLE testable point from the Live note — definitions, examples, contrasts, classifications, named processes, numerical claims — spread across the acts. Do not under-deliver.

ACT RHYTHM (mandatory):
- The FIRST act MUST be an investigation act (the player needs evidence before anyone can be cross-examined).
- ALTERNATE: investigation → testimony → investigation → testimony … A testimony act's lie may ONLY be busted with a card revealed in an EARLIER investigation act.
- Provide at least 3 investigation acts and at least 3 testimony acts.

INVESTIGATION ACT — the teaching beat:
- `narrative`: 2–4 Japanese sentences advancing the chapter's main plot (a scene in the campaign world: where the detective goes, who they meet, what they find).
- `evidence`: 2–4 cards (4 preferred when the lecture has the material). Each card = ONE meaty fact, 2–5 Japanese sentences. State the concept, then ground it with a concrete detail from the source: an example, a counter-example, a value, a contrast, or a named instance the teacher actually used. NEVER paste raw source text; rewrite as a clean fact paragraph (e.g. "教員はピジン言語の例として太平洋戦争中のヤシ語を挙げ、語彙は主に英語由来だが文法は太平洋諸語の影響を強く受けると説明した。さらに、ピジンが世代を超えて母語化したものがクレオールであり、両者は安定性で区別されると述べた。"). `title` = 6–30 char topic headline. `sourceRef` = the input alias (l1/s1/d1/...).

TESTIMONY ACT — the testing beat:
- `narrative`: 2–3 Japanese sentences moving the plot to the confrontation. MUST establish: (a) who this witness is (one-sentence background — origin/role in the locus), (b) why they are here / what they want from this encounter, (c) the moment they take the stand. The witness's motive must be inferable from this — it will drive their tone in testimony.
- `witnessName` (2–6 Japanese chars, invented unless reusing a bible cast member) + `witnessRole` (4–14 chars, fits the world's cast / community). Vary witnesses across acts when natural; reuse a bible character when it makes sense for their motivation.
- `testimony`: 3–5 statements spoken by that witness, of which EXACTLY ONE is a lie (`isFalse: true`). The lie CONTRADICTS one already-revealed evidence card; its `keyEvidenceId` is that card's id. The other (true) statements should ALSO be content-anchored — each is a different testable point the player should learn (not filler). All statements in plain Japanese witness speech, each under 140 chars.

CONTINUITY (MEMORY section): RE-EMPHASIZE recently-failed topics — make at least one lie touch one if present. DE-EMPHASIZE recently-mastered topics. Avoid reusing `recent_evidence_titles` verbatim.

WORLD & MOTIVATION (CAMPAIGN WORLD section, if present): every act's `narrative` happens INSIDE that world (its era/place/named locale/cast). The bible above lists each cast member's `背景`/`動機`/`利害` — when you reuse them, their behaviour in this chapter MUST flow from those. Even brand-new witnesses you invent for THIS chapter need a stated motive: in the testimony act's `narrative`, establish WHO this witness is (background sketch in 1 sentence), WHY they are at the scene / why they care, and WHAT they want from the encounter. A witness whose lie has no traceable motive is a failure — the lie should plausibly serve their interests (cover for an ally, protect reputation, conceal involvement, protect a payoff). The motive does NOT need to be revealed to the player as text — it just needs to shape the tone of their testimony and their `pressResponse`. Across the chapter, SEED the overarching hidden thread (暗线) with ONE subtle early hint — set `seedsMeta: true` on the single act that drops it, and keep it a passing detail (not resolved this chapter). Testable content stays exactly as rigorous; the world is dressing for the knowledge, never an excuse to invent facts. If NO world is given, use a neutral study framing — but characters still need motivation.

SESSION ALIGNMENT (授業計画): the Live note above is ONE lecture. Compare its actual content to the 授業計画 list and decide which 第N回 it corresponds to BY CONTENT (topic match), NOT by order. Output that number as top-level `sessionNum`. If the plan is empty or you genuinely cannot tell, use 0. Never guess by position.

COVERAGE (mandatory): for EVERY knowledge point listed above, ensure that the concept actually appears in some evidence card body OR in some testimony statement text. Then emit a `coverage` array mapping each covered point to where it landed. RULES:
- Every must-cover (★) point MUST appear, no exception. A chapter missing even one ★ point is rejected and the player has to retry.
- Total distinct points covered (★ + non-★ combined) MUST be ≥ {coverage_min}. So plan the chapter to fit at least {coverage_min} of the listed points.
- `placement` is either an evidence id ("e3") or a testimony id ("a4t3" — = act `a4`, statement `t3` in its testimony list).
- One point per coverage entry; you may cover one piece of content with multiple cards/statements but each coverage entry references a single placement.

Produce exactly this JSON shape (no comments, no extra fields):

{{
  "caseType": one of ["Exam Signal Case", "Concept Web Case", "Doubt Repair Case", "Contradiction Case", "Missing Link Case"],
  "difficulty": integer 1..5 (PREFER 2–3),
  "sessionNum": integer (the 第N回 this lecture matches by content; 0 if unsure),
  "briefing": "Japanese paragraph 60–220 chars summarising what this chapter investigates and what concepts it spans",
  "scenario": "4–6 Japanese sentences of narrative prologue: the chapter's hook, the situation, who's involved, what's at stake — set in the campaign world and rooted in the lecture content",
  "finalQuestion": "one specific Japanese question whose answer lies in the chapter's evidence",
  "acts": [
    {{ "id": "a1", "kind": "investigation", "title": "幕タイトル(〜16字)", "location": "舞台(任意)", "narrative": "2–4文の物語…", "seedsMeta": false,
       "evidence": [
         {{ "id": "e1", "title": "6–30字の見出し", "body": "2–5文の事実。概念→具体例/反例/数値/分類などで肉付けする。", "sourceRef": "l1" }},
         {{ "id": "e2", "title": "別の論点", "body": "2–5文の事実…", "sourceRef": "l1" }}
       ] }},
    {{ "id": "a2", "kind": "testimony", "title": "幕タイトル", "narrative": "2–3文で対決の場へ…", "seedsMeta": false,
       "witnessName": "ミナミ", "witnessRole": "ゼミ仲間",
       "testimony": [
         {{ "id": "a2t1", "text": "真実の証言（別の論点）…", "isFalse": false, "keyEvidenceId": "", "highlights": ["…逐語…"], "pressResponse": "2–3文で概念を教える。定義→具体例→対比、の順で踏み込む。" }},
         {{ "id": "a2t2", "text": "別の真実の証言…", "isFalse": false, "keyEvidenceId": "", "highlights": ["…"], "pressResponse": "2–3文で深掘り。" }},
         {{ "id": "a2t3", "text": "e1 と矛盾する嘘…", "isFalse": true, "keyEvidenceId": "e1", "highlights": ["…誤った語…"], "pressResponse": "2–3文で白を切る。論点をすり替えたり別の例を持ち出すが、嘘そのものは明かさない。" }}
       ] }},
    {{ "id": "a3", "kind": "investigation", "title": "…", "narrative": "…", "evidence": [ /* 2–4 cards, 2–5文 each */ ] }},
    {{ "id": "a4", "kind": "testimony", "title": "…", "witnessName": "…", "witnessRole": "…", "testimony": [ /* 3–5 statements, exactly 1 lie keyed to some earlier evidence */ ] }},
    {{ "id": "a5", "kind": "investigation", "title": "…", "narrative": "…", "evidence": [ /* … */ ] }},
    {{ "id": "a6", "kind": "testimony", "title": "…", "witnessName": "…", "witnessRole": "…", "testimony": [ /* … */ ] }}
  ],
  "coverage": [
    {{ "pointId": "k1", "placement": "e1" }},
    {{ "pointId": "k2", "placement": "a2t1" }},
    {{ "pointId": "k3", "placement": "e3" }}
    /* …continue until every ★ point and at least {coverage_min} total are listed */
  ],
  "caseLogic": {{
    "truth": "1〜3文。この事件の真相（実際に何が起きていたか）。",
    "culprit": "責任者（できれば世界観の登場人物名）。",
    "motive": "なぜそうしたか／なぜ嘘をつくか（手段・機会も織り込む）。",
    "redHerrings": ["もっともらしいが誤った手がかり1", "…2"],
    "deductionChain": ["突きつけた矛盾1 → 導かれること", "矛盾2 → …", "最後に finalQuestion へ答える結論"]
  }},
  "metaBeat": "本章が暗线に与えた一拍（プロットの planted beat と一致、既出の伏線を繰り返さず新しい角度で）"
}}

`sourceRef` MUST be one of the input aliases above (l1/l2/.../s1/.../d1/...). Every evidence `id` is unique across the whole chapter. Each lie's `keyEvidenceId` MUST be an evidence id revealed in an EARLIER investigation act. Each `highlights` entry MUST be a verbatim substring of its `text`. Every text field MUST follow the CONTENT-ONLY RULE in the system message."#,
        course = case.course_name,
        live = live_section,
        signals = signal_section,
        doubts = doubt_section,
        syllabus = syllabus_section,
        knowledge = knowledge_section,
        must_cover_list = must_cover_list,
        campaign = format_campaign_section(campaign, plan.arc_focus),
        memory = format_memory_section(memory),
        acts_min = targets.acts_min,
        acts_max = targets.acts_max,
        coverage_min = targets.coverage_min,
    )
}

// ─── Pass A: outline (推理 & 暗线 skeleton) ────────────────────────────────

pub(crate) fn detective_outline_system_prompt() -> &'static str {
    "You are the story architect for a 逆転裁判-style study-mystery. Your job in THIS pass is ONLY the logical skeleton of one chapter — no prose, no dialogue. A separate writer will turn your outline into the finished script, so the skeleton must be airtight.\n\nOUTPUT FORMAT — MANDATORY: a single JSON object, nothing else. Start with `{` end with `}`. No code fences, no commentary.\n\nWHAT MAKES A PROFESSIONAL MYSTERY SKELETON:\n- A real 明线 (the chapter case): a concrete TRUTH of what happened, a responsible party (culprit — PREFER a campaign bible cast member), a MOTIVE with means + opportunity, and 2–3 fair-play RED HERRINGS (plausible wrong readings that the evidence later eliminates).\n- A DEDUCTION CHAIN: the ordered steps by which busting the planted contradictions reconstructs the truth; the final step answers the chapter's question. Each step must be logically entailed by an evidence card or a busted lie — no leaps, no clue from nowhere.\n- TEACHING-BEFORE-TESTING: every testimony act's planted lie distorts a fact that an EARLIER investigation act teaches. In `actPlan`, name that fact in `lieAbout`.\n- The 暗线 (season conspiracy) advances by EXACTLY ONE planted beat — the campaign's current stage `setup`/`misdirection`. It is seeded as a passing detail in ONE act (`seedsMeta: true`), NOT resolved here, and must NOT repeat an already-dropped hook.\n\nHARD STRUCTURE (the writer pass + validator enforce these — plan for them now):\n- 6–8 acts, ALTERNATING investigation ↔ testimony, the FIRST act investigation; at least 3 investigation and 3 testimony acts.\n- Each testimony act has exactly ONE lie. The whole chapter has at least 3 lies total.\n- The chapter must cover at least the required number of knowledge points (★ must-cover ones are non-negotiable). Map them in `coveragePlan` and per-act `knowledgeIds`.\n\nCONTENT RULE: everything ties to TESTABLE lecture knowledge from the supplied Live note. No administrative trivia, no filenames/dates/codes, no invented facts. If the note is thin, build a tighter chapter rather than padding with filler — but still respect the structure targets the user prompt gives you.\n\nOUTPUT SHAPE:\n{\n  \"sessionNum\": integer (which 授業計画 第N回 this lecture matches by content; 0 if unsure),\n  \"caseType\": one of [\"Exam Signal Case\",\"Concept Web Case\",\"Doubt Repair Case\",\"Contradiction Case\",\"Missing Link Case\"],\n  \"caseLogic\": { \"truth\": \"…\", \"culprit\": \"…\", \"motive\": \"…\", \"redHerrings\": [\"…\",\"…\"], \"deductionChain\": [\"…\",\"…\",\"最後に問いへ答える結論\"] },\n  \"metaBeat\": \"この章が暗线に与える一拍（現段階の布石を、新しい角度で）\",\n  \"actPlan\": [\n    { \"index\": 1, \"kind\": \"investigation\", \"beat\": \"この幕で何を捜査し何を教えるか(1文)\", \"knowledgeIds\": [\"k1\",\"k3\"], \"seedsMeta\": false },\n    { \"index\": 2, \"kind\": \"testimony\", \"beat\": \"誰がなぜ証言台に立つか(1文)\", \"lieAbout\": \"歪める既習事実（どの知識点/捜査結果か）\", \"knowledgeIds\": [\"k2\"], \"witnessName\": \"…\", \"witnessRole\": \"…\", \"seedsMeta\": true }\n  ],\n  \"coveragePlan\": [\"k1\",\"k2\",\"k3\", \"…★を全て含み、必要数以上\"]\n}\nKeep all human-readable text in Japanese."
}

/// Build the Pass A (outline) user prompt — the same source/knowledge/world
/// context as the draft pass, but asking only for the logical skeleton.
pub(crate) fn detective_outline_user_prompt(
    case: &DetectiveCase,
    input: &[EvidenceInputEntry],
    memory: &DetectiveMemory,
    campaign: Option<&DetectiveCampaign>,
    syllabus: &[PlannedSession],
    knowledge: &[KnowledgePoint],
    targets: &GenTargets,
    plan: &ChapterPlan,
) -> String {
    let live = input
        .iter()
        .filter(|e| e.source_type == "live")
        .map(|e| truncate_chars(e.raw_content.replace('\r', "").trim(), 6500))
        .collect::<Vec<_>>()
        .join("\n---\n");
    let live = if live.trim().is_empty() {
        "(本文未抽出)".to_string()
    } else {
        live
    };
    let signals = input
        .iter()
        .filter(|e| e.source_type == "signal")
        .map(|e| truncate_chars(e.raw_content.trim(), 200))
        .collect::<Vec<_>>()
        .join("\n");
    let signals = if signals.trim().is_empty() {
        "(none)".to_string()
    } else {
        signals
    };
    let knowledge_section = if knowledge.is_empty() {
        "(知識点リスト未取得)".to_string()
    } else {
        knowledge
            .iter()
            .map(|p| {
                let star = if p.must_cover { "★" } else { " " };
                format!(
                    "- {star} `{}` {} — {}",
                    p.id,
                    p.label,
                    truncate_chars(p.gist.trim(), 80)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let must_cover: Vec<&str> = knowledge
        .iter()
        .filter(|p| p.must_cover)
        .map(|p| p.id.as_str())
        .collect();
    let syllabus_section = if syllabus.is_empty() {
        "(授業計画は未取得。sessionNum は 0)".to_string()
    } else {
        syllabus
            .iter()
            .map(|s| {
                let mode = if s.online {
                    "（オンライン）"
                } else {
                    ""
                };
                format!(
                    "- 第{}回{}: {}",
                    s.num,
                    mode,
                    truncate_chars(s.topic.trim(), 80)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        r#"Course: {course}

═══ LIVE LECTURE NOTE (primary source — the testable content) ═══
{live}

═══ Notifications (exam-context only) ═══
{signals}

═══ 授業計画 (match this lecture to its 第N回 by content) ═══
{syllabus}

═══ KNOWLEDGE POINTS (★ = must-cover) ═══
{knowledge}
must-cover ids: {must_cover}

═══ CAMPAIGN WORLD (this chapter is a beat inside it) ═══
{campaign}

═══ MEMORY (player history — drive continuity) ═══
{memory}

═══ THIS CHAPTER'S DIRECTION ═══
事件原型のヒント（今回はこの型で組み立てる）: {archetype}
暗线の担当段階: {arc_line}

═══ TASK ═══
Design the LOGICAL SKELETON (推理プロット) of ONE chapter for this lecture, as a {acts_min}–{acts_max}-act 逆転裁判 episode shaped by the archetype hint above. Decide the 明线 (truth / culprit / motive / red herrings / deduction chain). The CULPRIT should be one of the testimony witnesses (prefer a campaign cast member), and the deduction chain's steps should reference the evidence ids / testimony ids they rely on. Lay out the act plan (alternating investigation/testimony, first act investigation, ≥3 of each, exactly one lie per testimony act, ≥3 lies total), and plan knowledge-point coverage (every ★ + at least {coverage_min} total). Advance the 暗线 by the assigned stage's beat ONLY (do not jump ahead or resolve it), seeded in one act (`seedsMeta:true`) and not repeating an already-dropped hook. Output ONLY the JSON object specified in the system message."#,
        course = case.course_name,
        live = live,
        signals = signals,
        syllabus = syllabus_section,
        knowledge = knowledge_section,
        must_cover = if must_cover.is_empty() {
            "(なし)".to_string()
        } else {
            must_cover.join(", ")
        },
        campaign = format_campaign_section(campaign, plan.arc_focus),
        memory = format_memory_section(memory),
        archetype = plan.archetype,
        arc_line = match plan.arc_focus {
            Some(n) => format!("第{n}/{}段階（この段階の布石だけを進める）", plan.arc_total),
            None => "(暗线未設定)".to_string(),
        },
        acts_min = targets.acts_min,
        acts_max = targets.acts_max,
        coverage_min = targets.coverage_min,
    )
}

// ─── Pass C: editor critique / repair ──────────────────────────────────────

pub(crate) fn detective_editor_system_prompt() -> &'static str {
    "You are a senior script editor for a 逆転裁判-style study-mystery. You receive ONE chapter draft as JSON and audit it against a professional checklist. Your standard is high — a produced episode, not a rough cut.\n\nCHECKLIST:\n1. Logical consistency — the planted lie in each testimony act genuinely CONTRADICTS its `keyEvidenceId` card (revealed in an earlier investigation act). No lie keyed to a not-yet-shown card. The `caseLogic.deductionChain` actually follows from the evidence + busted lies and ends by answering `finalQuestion`.\n2. Motive traceability — every witness who lies has a plausible, inferable motive (cover an ally, protect reputation, conceal involvement, protect a payoff), consistent with the campaign cast's 背景/動機/利害 when a bible character is reused. `caseLogic.culprit` + `motive` are concrete.\n3. Fair play — red herrings are plausible but eliminable from the evidence; nothing is a cheat or a leap.\n4. 明线/暗线 — the chapter case resolves fully here; the 暗线 advances by exactly ONE seeded beat (`seedsMeta:true` on one act), consistent with the campaign's current stage and NOT repeating an already-dropped hook, NOT contradicting the world canon.\n5. Craft — scenes have subtext and voice, dialogue reveals under pressure rather than narrating, each act ends on a hook. press responses teach (for true statements) / deflect without confessing (for the lie).\n6. Content rule — everything is testable lecture knowledge; no admin trivia, filenames, dates-as-codes, invented facts; every ★ knowledge point is still covered.\n\nOUTPUT — MANDATORY, a single JSON object, nothing else:\n- If the draft already passes every check, output exactly: {\"ok\": true}\n- Otherwise output the FULL corrected chapter in the SAME JSON shape as the draft (all fields: caseType, difficulty, sessionNum, briefing, scenario, finalQuestion, acts[…], coverage[…], caseLogic{…}, metaBeat). Fix only what fails the checklist; preserve everything that already works, keep the same act count/kinds and the same knowledge coverage, and NEVER turn a correct testable fact into a wrong one. Keep all human-readable text in Japanese."
}

/// Build the Pass C (editor) user prompt: the draft JSON plus the consistency
/// anchors (must-cover points, campaign canon) the editor must respect.
pub(crate) fn detective_editor_user_prompt(
    draft_json: &str,
    outline_json: &str,
    campaign: Option<&DetectiveCampaign>,
    knowledge: &[KnowledgePoint],
    arc_focus: Option<u8>,
) -> String {
    let must_cover: Vec<&str> = knowledge
        .iter()
        .filter(|p| p.must_cover)
        .map(|p| p.label.as_str())
        .collect();
    format!(
        r#"═══ CAMPAIGN WORLD / CANON (the chapter must stay consistent with this) ═══
{campaign}

═══ APPROVED OUTLINE (the draft must stay faithful to this — same culprit / motive / per-act lie targets / structure / 暗线 stage) ═══
{outline}

═══ MUST-COVER knowledge points (all must remain covered) ═══
{must_cover}

═══ CHAPTER DRAFT (audit + repair this) ═══
{draft}

Run the checklist from the system message, and additionally verify the draft is FAITHFUL to the approved outline above (the realised culprit/motive, each testimony act's planted lie, the act structure, and the 暗线 stage must match the plan; the prose may be polished but the logic must not drift). If it all passes, return {{"ok": true}}. Otherwise return the full corrected chapter JSON (same shape)."#,
        campaign = format_campaign_section(campaign, arc_focus),
        outline = outline_json,
        must_cover = if must_cover.is_empty() {
            "(なし)".to_string()
        } else {
            must_cover
                .iter()
                .map(|m| format!("- {m}"))
                .collect::<Vec<_>>()
                .join("\n")
        },
        draft = draft_json,
    )
}

/// Render the campaign bible into a compact prompt section. When no campaign
/// exists yet (None), the case is generated world-free.
pub(crate) fn format_campaign_section(campaign: Option<&DetectiveCampaign>, arc_focus: Option<u8>) -> String {
    let Some(c) = campaign else {
        return "(この科目にはまだ世界観が設定されていない。中立的な学習シーンで構成すること)"
            .to_string();
    };
    let cast = if c.cast.is_empty() {
        "(未設定)".to_string()
    } else {
        c.cast
            .iter()
            .map(|m| {
                let mut block = format!("- {}（{}）", m.name, m.role);
                if !m.bond.trim().is_empty() {
                    block.push_str(&format!(" — {}", m.bond.trim()));
                }
                if !m.background.trim().is_empty() {
                    block.push_str(&format!("\n    背景: {}", m.background.trim()));
                }
                if !m.motivation.trim().is_empty() {
                    block.push_str(&format!("\n    動機: {}", m.motivation.trim()));
                }
                if !m.stake.trim().is_empty() {
                    block.push_str(&format!("\n    利害: {}", m.stake.trim()));
                }
                if !m.voice.trim().is_empty() {
                    block.push_str(&format!("\n    口調: {}", m.voice.trim()));
                }
                block
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let cast_log = if c.canon.cast_log.is_empty() {
        "(まだなし)".to_string()
    } else {
        c.canon
            .cast_log
            .iter()
            .rev()
            .take(8)
            .map(|e| format!("- {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let played = if c.chapters.is_empty() {
        "(まだ章は進んでいない — 序章にあたる)".to_string()
    } else {
        c.chapters
            .iter()
            .rev()
            .take(5)
            .map(|ch| format!("- {}", ch.title))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let relationships = if c.relationships.is_empty() {
        "(未設定)".to_string()
    } else {
        c.relationships
            .iter()
            .map(|r| {
                let tension = if r.tension.trim().is_empty() {
                    String::new()
                } else {
                    format!("（{}）", r.tension.trim())
                };
                format!("- {} ⇄ {}: {}{}", r.from, r.to, r.relation, tension)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    // The 暗线 stage this chapter should plant toward. Prefer the chapter's own
    // arc position (`arc_focus`, derived from its place in the season) so the
    // serialized story stays coherent regardless of play order; fall back to the
    // lowest not-yet-unlocked stage when no position is supplied.
    let current_stage = match arc_focus {
        Some(n) => c
            .meta_arc
            .iter()
            .find(|r| r.stage == n)
            .or_else(|| c.meta_arc.last()),
        None => c
            .meta_arc
            .iter()
            .find(|r| !r.unlocked)
            .or_else(|| c.meta_arc.last()),
    };
    let arc_total = c.meta_arc.len().max(1);
    let arc_section = match current_stage {
        Some(stage) => {
            // Antagonist presence escalates across the season: early chapters
            // only leave traces, late chapters bring the black hand to the fore.
            let presence = match (stage.stage as usize * 4) / arc_total {
                0 => "黒幕の存在はまだ痕跡のみ（噂・物・名前が一度かすめる程度）。",
                1 => "黒幕の影が近づく（代理人や利害関係者が一人、脇で動く）。",
                2 => "黒幕の手が事件に絡む（その思惑が今回の出来事に直接影響する）。",
                _ => "黒幕（またはその代理人）が前面に出て探偵と対峙しうる段階。",
            };
            let mut block = format!(
                "今、進めるべき暗线の段階: 第{}/{}段階「{}」\n    {}",
                stage.stage, arc_total, stage.title, presence
            );
            if !stage.setup.trim().is_empty() {
                block.push_str(&format!("\n    埋めるべき布石: {}", stage.setup.trim()));
            }
            if !stage.misdirection.trim().is_empty() {
                block.push_str(&format!(
                    "\n    効かせる誤導: {}",
                    stage.misdirection.trim()
                ));
            }
            block
        }
        None => "(段階未設定 — meta-mystery を一度だけ匂わせる)".to_string(),
    };
    // Already-dropped hooks so chapters vary their hints instead of repeating.
    let dropped = if c.canon.dropped_hooks.is_empty() {
        "(まだなし)".to_string()
    } else {
        c.canon
            .dropped_hooks
            .iter()
            .rev()
            .take(8)
            .map(|h| format!("- {}", h.hook))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let facts = if c.canon.facts.is_empty() {
        "(まだなし)".to_string()
    } else {
        c.canon
            .facts
            .iter()
            .rev()
            .take(12)
            .map(|f| format!("- {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "世界観ラベル: {label}\n舞台設定: {setting}\nキャッチコピー: {tagline}\n登場人物（この章でも最低1人を証人として再登場させ、既定の動機/利害/口調に従わせること）:\n{cast}\n人物相関:\n{relationships}\nこれまでの登場履歴（続きとして矛盾なく描く）:\n{cast_log}\n\n大きな暗线（meta-mystery — 全章を貫く隠された真相。本章では解決せず、下記の“今進めるべき段階”だけを布石として織り込む）:\n{meta}\n{arc}\n\n世界の正典（これと矛盾してはならない既定事実）:\n{facts}\nすでに投下済みの伏線（繰り返さず、新しい角度で）:\n{dropped}\n\n進行度: {progress}/100\nこれまでの章:\n{played}",
        label = c.world_label,
        setting = c.setting,
        tagline = if c.tagline.trim().is_empty() { "(なし)" } else { c.tagline.trim() },
        cast = cast,
        relationships = relationships,
        cast_log = cast_log,
        meta = c.meta_mystery,
        arc = arc_section,
        facts = facts,
        dropped = dropped,
        progress = c.meta_progress,
        played = played,
    )
}

/// Render the persisted memory into a compact prompt section. Keeps only
/// recent entries so the AI isn't drowned.
pub(crate) fn format_memory_section(memory: &DetectiveMemory) -> String {
    if memory.mistakes.is_empty()
        && memory.mastered.is_empty()
        && memory.recent_evidence_titles.is_empty()
    {
        return "(まだ過去のセッション記録はない)".to_string();
    }
    let mistakes = memory
        .mistakes
        .iter()
        .rev()
        .take(6)
        .map(|m| format!("- {}（{}）", m.topic, m.course_name))
        .collect::<Vec<_>>()
        .join("\n");
    let mastered = memory
        .mastered
        .iter()
        .rev()
        .take(8)
        .map(|m| format!("- {}（{}）", m.topic, m.course_name))
        .collect::<Vec<_>>()
        .join("\n");
    let recent = memory
        .recent_evidence_titles
        .iter()
        .rev()
        .take(10)
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mistakes_block = if mistakes.is_empty() {
        "(なし)".to_string()
    } else {
        mistakes
    };
    let mastered_block = if mastered.is_empty() {
        "(なし)".to_string()
    } else {
        mastered
    };
    let recent_block = if recent.is_empty() {
        "(なし)".to_string()
    } else {
        recent
    };
    format!(
        "Recently failed topics (RE-EMPHASIZE — at least one lie should touch these):\n{}\n\nRecently mastered topics (DE-EMPHASIZE — don't repeat as the lie):\n{}\n\nRecently used evidence titles (find new angles, don't reuse verbatim):\n{}",
        mistakes_block, mastered_block, recent_block,
    )
}

pub(crate) fn campaign_bible_system_prompt() -> &'static str {
    r#"You are the lead writer for a long-running courtroom-mystery game in the spirit of 逆転裁判 (Ace Attorney). You are designing the CAMPAIGN BIBLE — the persistent world (世界観) that every future chapter of one university course lives inside.

═══ THE GOLDEN RULE — anchor to a SIGNATURE HISTORICAL LOCUS ═══

The world is NOT "a vaguely-themed setting matching the subject". It is **the most iconic, historically/culturally representative time-and-place that the subject is associated with** — the specific moment a textbook would cite as the topic's center of gravity. Reach for the concrete locus, not the generic theme.

Good vs. bad anchoring:
- 米国黒人英語(AAVE) → ❌「19世紀のアメリカ」 / ✅「1960年代公民権運動下のミシシッピ・デルタの綿花町」or「1920年代ハーレム・ルネサンス期のニューヨーク」
- 確率論 → ❌「賭博の街」 / ✅「17世紀パスカルとフェルマーが文通した賭博問題のパリ・サロン」
- 量子力学 → ❌「物理学の街」 / ✅「1927年ソルベイ会議直前のコペンハーゲン、ボーア研究所」
- 古代中国法制 → ❌「中華風の都」 / ✅「商鞅変法下の戦国秦・咸陽」
- ピジン/クレオール言語 → ❌「多言語の港」 / ✅「19世紀末ハワイの製糖プランテーションと寄せ集めの労働者集落」

Specificity is non-negotiable. Name the **decade or specific historical event window** + **a real-feeling named locale** + **a concrete community/social structure**. If you cannot identify a signature locus, look harder — every academic subject has one.

═══ CHARACTERS NEED BACKGROUND, MOTIVATION, AND STAKES ═══

Every cast member is a person, not a label. For each, write:
- `background` (2–3 sentences): where they came from, what they've done before the campaign starts, what shaped them. NOT a role tag — a concrete mini-bio.
- `motivation` (1 sentence): what they WANT right now in this world. Every action they take in any chapter should be traceable here.
- `stake` (1 sentence): what they LOSE if the truth comes out / things go wrong. This is why they may lie, evade, or push back when cross-examined.
- `bond` (1 short phrase): their relation to the protagonist or to the meta-mystery.

═══ THE META-MYSTERY NEEDS A FACE ═══

The 暗线 is not just "a hidden truth". Name a **concrete antagonist force** — a person, a society, a guild, an institution — and give IT its own motivation (why are they hiding the truth? what do THEY want?). This makes the conspiracy feel real and the meta-arc reveals concrete.

═══ THE META-ARC IS A STORYBOARD, NOT A SUMMARY ═══

The 暗线 unfolds across the whole season. Design it like a professional serialized-mystery writer: each of the 4 stages is a STORYBOARD BEAT, not a vague summary. For each stage author THREE things that future chapters will execute:
- `setup`: the concrete hook/clue chapters at this stage should plant (a recurring object, a slip of the tongue, an inconsistent record, a name that keeps surfacing). Plantable as a passing detail inside an ordinary chapter.
- `misdirection`: the plausible-but-wrong reading that keeps the audience from guessing the truth too early — fair-play misdirection, not a cheat.
- `reveal`: what the audience actually learns when this stage lands (player-facing text).
Each stage must BUILD ON the previous one: (1) faint hint → (2) deepening clue that complicates stage 1 → (3) twist that recontextualises stages 1–2 → (4) full payoff naming the antagonist's identity + motivation. The reveals must be logically entailed by the setups (no clue appears from nowhere; no reveal contradicts an earlier stage).

═══ RELATIONSHIPS GIVE THE WORLD TENSION ═══

Cast members are not isolated. Author a small relationship web (2–4 edges) among the cast and the antagonist force — alliances, rivalries, debts, secret collaborations — each with the underlying tension that could erupt in a chapter.

═══ OUTPUT — return ONE JSON object, nothing else ═══
{
  "worldLabel": "8–22 char Japanese label naming the SIGNATURE LOCUS (era + place), e.g. 「1965年・ミシシッピ綿花町」「1927年・コペンハーゲン」",
  "setting": "3–5 Japanese sentences establishing the locus: the specific historical moment, the named place, the community structure, and WHY mysteries happen here. Anchor in real history that connects to the course subject.",
  "tagline": "one short Japanese hook line, under 28 chars — should evoke the SPECIFIC locus, not be generic",
  "metaMystery": "3–5 Japanese sentences. Name the antagonist force (who/what is hiding the truth), what they are concealing, and WHY they are concealing it (their motivation). The conspiracy must be thematically tied to the course's core ideas, and must be seedable in early chapters and payable in the finale.",
  "cast": [
    {
      "name": "2–6 char Japanese name (fits the locus's culture)",
      "role": "役回り e.g. 相棒/ライバル/黒幕候補/語り部/情報屋",
      "bond": "one phrase: their relation to the protagonist or the meta-mystery",
      "background": "2–3 Japanese sentences — concrete past, where they came from, what shaped them",
      "motivation": "1 Japanese sentence — what they WANT right now",
      "stake": "1 Japanese sentence — what they LOSE if the truth comes out",
      "voice": "口調カード — 一人称・語尾・口癖など、再登場時に声を一致させるための短いメモ"
    }
  ],
  "relationships": [
    { "from": "人物名/黒幕勢力", "to": "人物名/黒幕勢力", "relation": "関係(兄弟/師弟/対立/秘密の協力者…)", "tension": "燻る火種を1フレーズで" }
  ],
  "metaArc": [
    { "title": "短い見出し(〜12字)", "setup": "この段階で各章が仕込むべき具体的な布石", "misdirection": "観客を真相から逸らす“もっともらしい誤読”", "reveal": "1–2 Japanese sentences — この段階で観客が知る事実(プレイヤー向け表示文)", "sessionBand": "担当する第N回の帯(例 \"1-3\")" }
  ],
  "finale": "3–5 Japanese sentences — the grand epilogue shown when the campaign is 100% complete. The conclusive resolution of the metaMystery: who/what was behind it, what they wanted, how the detective resolves it, the closing image."
}

Provide 2–3 cast members; one of them should plausibly be the antagonist force's local agent or someone whose stake aligns with the meta-mystery. Provide 2–4 `relationships`. `metaArc` = EXACTLY 4 ordered entries with the storyboard fields above: (1) faint hint, (2) deepening clue, (3) twist that recontextualises earlier chapters, (4) full payoff revealing the antagonist's identity + motivation; distribute `sessionBand` to roughly quarter the season. `finale` lands AFTER stage 4 and must pay off the setups planted in stages 1–4. Keep everything in Japanese. Names, places, dates, and references must FEEL historically grounded — avoid totally invented place names when a real locus exists. Never invent facts that contradict the discipline."#
}

pub(crate) fn campaign_bible_user_prompt(course_name: &str, input: &[EvidenceInputEntry]) -> String {
    let live: Vec<String> = input
        .iter()
        .filter(|e| e.source_type == "live")
        .map(|e| truncate_chars(e.raw_content.replace('\r', "").trim(), 1200))
        .collect();
    let signals: Vec<String> = input
        .iter()
        .filter(|e| e.source_type == "signal")
        .map(|e| truncate_chars(e.raw_content.trim(), 160))
        .collect();
    let live_block = if live.is_empty() {
        "(none)".to_string()
    } else {
        live.iter()
            .enumerate()
            .map(|(i, b)| format!("[{}]\n{}", i + 1, b))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let signal_block = if signals.is_empty() {
        "(none)".to_string()
    } else {
        signals.join("\n")
    };
    format!(
        r#"Course: {course}

═══ COURSE CONTENT (read this to infer the SUBJECT, then build a world around it) ═══

Live lecture notes (what the teacher actually taught):
{live}

Notifications (exam scope / format intel):
{signals}

═══ TASK ═══
Infer the academic SUBJECT of this course from the content above (history? statistics? linguistics? chemistry? law? …). Then design the CAMPAIGN BIBLE: a mystery world whose era/place/genre is DERIVED FROM that subject (see the system rules — American Revolution → 1770s colonial town, etc.). Establish a recurring cast and an overarching hidden conspiracy (暗线) that future chapters will unravel. Return the JSON object exactly as specified in the system message — Japanese, no extra fields, no prose."#,
        course = course_name,
        live = live_block,
        signals = signal_block,
    )
}

