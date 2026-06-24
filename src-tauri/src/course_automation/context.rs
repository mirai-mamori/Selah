//! Token-bounded context assembly for SenseA.
//!
//! SenseA keeps three distinct memory tiers:
//! - the persistent artifact/document ledger is the complete audit history;
//! - `AgentCourseAnalysis` is compact working memory for the next decision;
//! - newly analyzed documents are episodic deltas processed once.
//!
//! Full history must not be replayed to the model. New evidence is folded into
//! working memory in bounded batches, while the ledger remains available for
//! deterministic reuse, retries, and provenance.

use super::items::Item;
use super::{
    AgentCourseAnalysis, AnalysisDocument, ArchivedGroup, DocumentAnalysis, PrintCandidate,
    SeatConclusion, SourceEvent,
};
use serde::Serialize;
use serde_json::Value;

const SUMMARY_DELTA_TOKEN_BUDGET: usize = 24_000;
const SUMMARY_MEMORY_TOKEN_BUDGET: usize = 8_000;

pub(super) const INDIVIDUAL_SYSTEM_PROMPT: &str =
    "あなたは大学授業を継続的に支援する SenseA です。今は資料を一件だけ読み、後段の全体判断に必要な索引を作ります。他の資料を推測せず、この資料に明記された事実だけを記録してください。\n\
出力方針:\n\
- summary: この資料に明記された客観的事実を1文・原則120文字以内で記述してください。学生にとっての意味づけ・解釈・評価・推奨・感想は書かず、書かれている事実そのもの(何が・いつ・どうなるか)を述べます。授業名・大学名・資料名の復唱、本文の言い換え、背景説明は不要です。\n\
- findings: 学生が行動・注意・判断に使える具体的事実だけを最大3件。期限、変更点、提出条件などを優先し、重複させないでください。\n\
- 期限・日時・締切・授業回・公開や提出のタイミングなど時間に関わる事項は、summary と findings に必ずその時刻情報(年月日・時刻・第○回など、資料に書かれた粒度のまま)を明記してください。後段が現在時刻と照合し、すでに失効した情報かどうかを判断できるようにするためです。時刻が特定できない場合は、その旨(例:日付未記載)も含めて記録してください。\n\
- seatEvidence: 座席・学籍番号との対応を判断できる根拠だけを、列・番目・グループ名・前後左右の向きなど座席位置を特定できる要素を省略せず記録してください(後段が完全な座席表現を組み立てられるようにするため。教室名や授業回は不要です)。座席と無関係なら空配列にしてください。\n\
- printInstruction: この資料を紙に印刷して書き込む必要があるかを、明示の有無に関わらず内容から判断して短く記録します。「印刷」という語がなくても、次のような場合は印刷が必要とみなしてください: 印刷・手書き・紙での提出・持参が明示されている; 課題本文の中に印刷指示が埋もれている; あるいは本文に記入欄・解答欄・空欄・コメント欄・署名欄や、設問+空白(穴埋め・記述スペースなど)といった、紙に書き込んで使うことが前提の構造がある。逆に、画面で読むだけ・オンラインで完結する・配布スライドや参考資料に過ぎないものは記録しません。判断の根拠(例:記入欄あり/穴埋め形式)も一言添え、構造的な手掛かりのない当てずっぽうは避けてください。\n\
- triggerDecision: この資料単体の影響度で判断します。これは「記憶を今すぐ更新すべきか」を決める信号で、immediate なら後段は他資料の蓄積を待たず即座に全体記憶を更新します。次のいずれかに該当するなら必ず immediate にしてください: (1) 新たな締切・提出期限・期限の前倒しや延長など、放置すると間に合わなくなる時間的事項; (2) 休講・補講・教室変更・開始時刻や日程の変更; (3) 試験・小テスト・出席確認・口頭発表など評価に直結する告知; (4) 座席・班・担当の指定や変更; (5) 既存の前提・以前の指示を覆す訂正・撤回・中止; (6) 提出方法や持参物など、当日までに準備が要る必須の行動。これらに当たらず、単体では結論できず今後の資料と照合してはじめて意味が定まる未解決事項なら observe。記録には残すが特段の行動を要しない一般連絡・補足資料なら routine。語句の一致ではなく内容の実際の影響で判断し、迷う場合で学生の不利益が大きいものは immediate を優先してください。失効しているかどうかの最終判定は後段に委ね、ここでは時刻情報を残すことを優先します。\n\
- observationContext: 将来の資料と照合しないと結論できない未解決事項だけ。今回だけで完結する内容や summary の重複は書かないでください。\n\
必ず次の JSON のみを返してください:\n\
{\"summary\":\"...\",\"findings\":[\"...\"],\"seatEvidence\":[\"...\"],\"printInstruction\":\"...\",\"triggerDecision\":\"immediate|observe|routine\",\"observationContext\":\"...\"}";

pub(super) const SUMMARY_SYSTEM_PROMPT: &str =
    "あなたは大学授業を継続的に支援する SenseA です。目的は「授業の歴史を説明すること」ではなく、学生が今知るべきことと、次回更新に必要な記憶を短く維持することです。\n\
入力の役割:\n\
- previousCourseAnalysis: 前回の圧縮済み作業記憶。追記先ではなく、現在の証拠に基づいて全面的に書き直す草稿です。standingContext は現在有効な記憶、archivedContext は失効・完了した過去の記憶を見出しでまとめた履歴です。どちらもあなたが毎回書き直して維持します。入力サイズを抑えるため archivedContext は一部だけ届くことがあります。届いていない過去項目は外部の監査記憶に保持されているので、「入力に無い=存在しない」と解釈せず、今回の新証拠と届いた作業記憶だけで更新してください。新しい資料を解釈するとき、過去に何があったか・何と関連しそうかの手掛かりとして archivedContext を必ず参照してください。失効済みの事項が新情報で再び現役化した場合に限り、archivedContext から取り出して新しい standingContext に戻してかまいません。\n\
- newOrChangedDocuments: 今回追加・更新された個別まとめ。今回の判断に使う新しい証拠です。\n\
- sourceEvents: Luna 側で見えた資料ライフサイクルの変化です。removed は前回まで存在した資料が現在の一覧に無いこと、changed は同じ題名/ファイル名の資料が新しい版に置き換わったことを示します。previousSummary は古い版についての短い記録なので、現在の事実として復唱せず、既存記憶の撤回・更新・保留判断にだけ使ってください。\n\
- currentLocalTime: 期限や現在性を判断する基準時刻です。\n\
出力方針:\n\
- summary: 学生が今最初に知るべき最優先の結論1〜2件だけを、2文・160文字以内で記述してください。大学名、学部、授業名、担当者、教室、座席、印刷対象、授業回の一覧、配布済み資料の列挙は禁止です。複数の締切やルールを一段落へ詰め込まないでください。\n\
- findings: これは表示用の摘要リストではありません。学生が近いうちに対応する、または明示的に知っておく必要がある短期の介入項目だけを {\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD\",\"action\":false} のオブジェクト配列で記録します。(A)学生が実際に手を動かす必要がある行動(提出・準備・申込・出席など)は action:true にします。action:true は TODO リストへ送られるため、Plus の保留中表示では重複表示されません。(B)行動は不要だが近いうちに明示的に知るべき通知・変更・連絡(教室や時間の変更、休講・補講、次回小テストの範囲、配布や公開の予定、訂正のお知らせなど)は action:false にします。普通の資料要約、授業回や配布済み資料の列挙、summary の言い換え、座席情報、印刷候補、長期ルールは findings に入れてはいけません。座席は seat、印刷は printCandidates、恒常的なルール・前提・背景・使用教材・一般的な運用方針は standingContext に入れてください。該当がなければ空配列 [] にし、無理に作らないこと。各 text は単独で理解できる短い1文とし、summary と重複させないでください。expiresAt にはその項目が無意味になる日付(締切日・対象授業回の日付・イベント当日など)を YYYY-MM-DD で入れ、過ぎたものは出力しないでください。特定の失効日がなければ空文字 \"\" にします。通知・連絡・お知らせなど『知っておくだけ』のものは、たとえ重要でも必ず action:false にし、TODO 化させないでください。\n\
- standingContext: この授業の運用に明確に結びつく記憶だけを、{\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD\",\"urgent\":false} のオブジェクト配列で記録します。text は、なぜこの授業に関係するか(対象の課題・回・ルール・教員の指示など)が項目自身から分かる形で書いてください。expiresAt には、その記憶が有効でなくなる日付(締切日・対象授業回の日付・イベント当日など、それを過ぎたら不要になる日)を YYYY-MM-DD で入れます。expiresAt を過ぎた項目は standingContext に残さず、下記の archivedContext へ要約して移してください(システムも currentLocalTime と照合して自動で移しますが、まずはあなたが整理することを優先します)。学期を通じて常に有効なルールなど、特定の失効日を持たない記憶は expiresAt を空文字 \"\" にしてください(推測で適当な日付を入れないこと)。urgent は、時間的に切迫し学生が今すぐ確認・行動すべき記憶なら true にします: 間近の締切・提出、休講や補講や教室変更や日程変更、試験や小テストや出席確認、座席や班の指定や変更、以前の前提を覆す訂正・撤回・中止など。落ち着いて参照すればよい通常のルールや背景は false にしてください。urgent な事項は新しい資料で動いたときに記憶を即時更新すべき種類でもあります。件数の上限はありません。残すべきものは漏らさず、逆にこの授業と関係が薄い一般論・他授業の事柄・一時的な連絡は入れないでください。条件は「次回以降の判断で失うと困る」「現在も有効」の両方を満たすこと。表示用の summary/findings を復唱しないでください。\n\
- archivedContext: 失効・完了した過去の記憶を、関連するものごとに見出し(label)でまとめた履歴です。[{\"label\":\"完了した課題\",\"items\":[\"...\"]}] の形で、各 label の下に短い1行の items を並べます。例:提出が終わった課題は「完了した課題」という label の下に「第3回レポート(6/10提出)」のような小項目としてまとめます。原文をそのまま残さず、要点だけに圧縮し、似た項目は1項目に統合してください(完了した課題・終了したイベント・過去の変更・撤回された連絡などを、それぞれ適切な label にまとめる)。前回の archivedContext を引き継ぎつつ、新たに失効した standingContext を該当する label に畳み込み、全体を簡潔に保ってください。将来の資料と関連しそうにない些末な項目は省いて構いません。\n\
- currentLocalTime より前の期限、完了済み事項、新情報で置き換えられた事項は summary/findings/standingContext に出力してはいけません。\n\
- 座席情報は seat にだけ記録し、summary/findings へ出力してはいけません。assignment には、学生本人がそのまま着席するための座席位置だけを自然な日本語でまとめてください(例:「7列目・前から8番目」)。列・番目・前後左右など本人の位置を特定する情報は省かず、判明しない要素だけを省きます。グループ名、班名、他学生の学籍番号・名前・メンバー一覧は assignment に絶対に含めず、判断根拠として必要な場合だけ evidence に分離してください。座席指定がまったく不明なときに限り「指定なし」とします。教室名や授業回も座席表現に含めないでください。\n\
- 印刷対象は printCandidates にだけ記録し、これから紙で使う必要があるものだけに限定します。個別まとめの printInstruction で「紙に書き込む/印刷が前提」と判断されたファイル(記入欄・空欄・設問構造などを含む)を対象にしてください。currentLocalTime を基準に、締切・提出・使用機会がすでに過ぎていて今印刷しても意味がないもの(過去回の配布物、提出済みの課題、終了したワークシートなど)は含めないでください。filename は実際のファイル名、reason には印刷が必要と判断した根拠(例:解答欄あり)、confidence(0〜1)を付けます。さらに category には、同種の印刷物をまとめる短い種別名(例:ワークシート / 小テスト / 出席カード / 記入式レポート)を付けてください。ユーザーはこの category 単位で印刷を許可するため、同じ性質の印刷物には一貫した同じ category 名を使ってください。summary/findings へ重複させないでください。\n\
悪い summary の例: 「○○大学○○学部の授業で、第1回は…第2回は…資料が配布済み…」。これは履歴の転載なので禁止です。\n\
良い summary の例: 「6月18日17:00までの全体課題提出が最優先。第9回から出席確認時間が毎回変わるため、授業中の案内に注意する。」\n\
出力前に必ず確認してください: summary が160文字以内か、summary と findings が重複していないか、期限切れ事項を残していないか、座席・印刷を summary/findings に混ぜていないか、履歴を列挙していないか。\n\
必ず次の JSON のみを返してください:\n\
{\"summary\":\"...\",\"findings\":[{\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD または空文字\",\"action\":false}],\"standingContext\":[{\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD または空文字\",\"urgent\":false}],\"archivedContext\":[{\"label\":\"完了した課題\",\"items\":[\"...\"]}],\"seat\":{\"assignment\":\"...\",\"evidence\":[\"...\"],\"confidence\":0.0},\"printCandidates\":[{\"filename\":\"実際のファイル名\",\"reason\":\"...\",\"confidence\":0.0,\"category\":\"ワークシート\"}]}";

pub(super) const ORGANIZE_SYSTEM_PROMPT: &str =
    "あなたは大学授業の資料整理を担当する SenseA です。すでに一件ずつ要約済みの資料一覧(documents: 各 id・kind・title・summary)を受け取ります。これらをフォルダ分けするためのグループにまとめてください。可能であれば courseSchedule(授業計画。第○回ごとの日付やテーマ)を手掛かりに使ってください。\n\
方針:\n\
- 原則として、できる限り全ての資料をいずれかのグループに入れてください。中途半端に多くを未分類のまま残さないこと。\n\
- 授業回(第○回)に結び付く資料は、回ごとのグループにまとめるのを最優先にしてください。ラベルは必ず『第01回』『第03回』のように二桁ゼロ詰めの『第NN回』形式に統一します(既存の回フォルダと一致させるため)。\n\
- title だけでなく summary の内容を根拠にしてください。日付だけのファイル名(例: ライブノート・板書、配付プリント)は、summary の内容や日付を courseSchedule と照合して、対応する第NN回に入れてください。\n\
- どの回にも結び付かない資料は、主題(『個人レポート』『小テスト』など)または種類(『教材』『課題』)でまとめます。スラッシュや記号はラベルに使わないでください。\n\
- 1グループは原則2件以上。同じ資料(id)を複数グループに入れないこと。どうしても分類できない資料だけ省いてかまいません。\n\
必ず次の JSON のみを返してください:\n\
{\"groups\":[{\"label\":\"第03回\",\"fileIds\":[\"id1\",\"id2\"]}]}";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IndividualInput<'a> {
    pub course_id: &'a str,
    pub course_name: &'a str,
    pub student: &'a Value,
    pub document: CompactDocument<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CompactDocument<'a> {
    pub kind: &'a str,
    pub title: &'a str,
    pub filename: &'a str,
    pub content: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SummaryInput<'a> {
    pub current_local_time: String,
    pub course_id: &'a str,
    pub course_name: &'a str,
    pub student: &'a Value,
    pub previous_course_analysis: &'a AgentCourseAnalysis,
    pub new_or_changed_documents: Vec<CompactAnalysis<'a>>,
    pub source_events: Vec<CompactSourceEvent<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CompactAnalysis<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub filename: &'a str,
    pub summary: &'a str,
    pub findings: &'a [String],
    pub seat_evidence: &'a [String],
    pub print_instruction: &'a str,
    pub trigger_decision: &'a str,
    pub observation_context: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CompactSourceEvent<'a> {
    pub id: &'a str,
    pub event: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub filename: &'a str,
    pub previous_summary: &'a str,
    pub detail: &'a str,
}

impl<'a> From<&'a AnalysisDocument> for CompactDocument<'a> {
    fn from(document: &'a AnalysisDocument) -> Self {
        Self {
            kind: &document.kind,
            title: &document.title,
            filename: &document.filename,
            content: &document.content,
        }
    }
}

impl<'a> From<&'a DocumentAnalysis> for CompactAnalysis<'a> {
    fn from(analysis: &'a DocumentAnalysis) -> Self {
        Self {
            id: &analysis.id,
            kind: &analysis.kind,
            title: &analysis.title,
            filename: &analysis.filename,
            summary: &analysis.summary,
            findings: &analysis.findings,
            seat_evidence: &analysis.seat_evidence,
            print_instruction: &analysis.print_instruction,
            trigger_decision: &analysis.trigger_decision,
            observation_context: &analysis.observation_context,
        }
    }
}

impl<'a> From<&'a SourceEvent> for CompactSourceEvent<'a> {
    fn from(event: &'a SourceEvent) -> Self {
        Self {
            id: &event.id,
            event: &event.event,
            kind: &event.kind,
            title: &event.title,
            filename: &event.filename,
            previous_summary: &event.previous_summary,
            detail: &event.detail,
        }
    }
}

pub(super) fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(3).max(1)
}

pub(super) fn log_request_size(label: &str, instructions: &str, input: &str) {
    log::info!(
        "[course_automation] {} prompt size: instructions={} tokens, input={} tokens",
        label,
        estimate_tokens(instructions),
        estimate_tokens(input)
    );
}

pub(super) fn compact_student_profile(student: &Value) -> Value {
    let mut compact = serde_json::Map::new();
    for key in [
        "student_id",
        "name",
        "name_en",
        "faculty",
        "department",
        "major",
        "class",
    ] {
        if let Some(value) = student.get(key).filter(|value| !value.is_null()) {
            compact.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(compact)
}

pub(super) fn prompt_course_analysis(analysis: &AgentCourseAnalysis) -> AgentCourseAnalysis {
    prompt_course_analysis_with_budget(analysis, SUMMARY_MEMORY_TOKEN_BUDGET)
}

fn prompt_course_analysis_with_budget(
    analysis: &AgentCourseAnalysis,
    budget: usize,
) -> AgentCourseAnalysis {
    let mut prompt = AgentCourseAnalysis {
        summary: analysis.summary.clone(),
        seat: SeatConclusion {
            assignment: analysis.seat.assignment.clone(),
            confidence: analysis.seat.confidence,
            ..Default::default()
        },
        ..Default::default()
    };
    add_items_with_budget(&mut prompt, &analysis.findings, budget, |memory| {
        &mut memory.findings
    });
    add_items_with_budget(&mut prompt, &analysis.standing_context, budget, |memory| {
        &mut memory.standing_context
    });
    add_print_candidates_with_budget(&mut prompt, &analysis.print_candidates, budget);
    add_seat_evidence_with_budget(&mut prompt, &analysis.seat.evidence, budget);
    add_archive_with_budget(&mut prompt, &analysis.archived_context, budget);
    prompt
}

fn prompt_memory_tokens(analysis: &AgentCourseAnalysis) -> usize {
    serde_json::to_string(analysis)
        .map(|raw| estimate_tokens(&raw))
        .unwrap_or(usize::MAX)
}

fn item_prompt_priority(item: &Item) -> (usize, bool) {
    let urgent_or_action = item.flags.urgent || item.flags.action;
    if urgent_or_action {
        (0, item.expires_at.is_empty())
    } else if !item.expires_at.is_empty() {
        (1, false)
    } else {
        (2, true)
    }
}

fn add_items_with_budget(
    prompt: &mut AgentCourseAnalysis,
    items: &[Item],
    budget: usize,
    facet: fn(&mut AgentCourseAnalysis) -> &mut Vec<Item>,
) {
    let mut indexed = items.iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by_key(|(index, item)| (item_prompt_priority(item), *index));
    for (_, item) in indexed {
        let mut next = prompt.clone();
        facet(&mut next).push(item.clone());
        if prompt_memory_tokens(&next) <= budget {
            *prompt = next;
        }
    }
}

fn add_seat_evidence_with_budget(
    prompt: &mut AgentCourseAnalysis,
    evidence: &[String],
    budget: usize,
) {
    for item in evidence {
        let mut next = prompt.clone();
        next.seat.evidence.push(item.clone());
        if prompt_memory_tokens(&next) <= budget {
            *prompt = next;
        }
    }
}

fn add_print_candidates_with_budget(
    prompt: &mut AgentCourseAnalysis,
    candidates: &[PrintCandidate],
    budget: usize,
) {
    let mut indexed = candidates.iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        right
            .confidence
            .total_cmp(&left.confidence)
            .then_with(|| left_index.cmp(right_index))
    });
    for (_, candidate) in indexed {
        let mut next = prompt.clone();
        next.print_candidates.push(candidate.clone());
        if prompt_memory_tokens(&next) <= budget {
            *prompt = next;
        }
    }
}

fn add_archive_with_budget(
    prompt: &mut AgentCourseAnalysis,
    archived_context: &[ArchivedGroup],
    budget: usize,
) {
    for group in archived_context {
        for item in &group.items {
            let mut next = prompt.clone();
            if let Some(existing) = next
                .archived_context
                .iter_mut()
                .find(|existing| existing.label == group.label)
            {
                existing.items.push(item.clone());
            } else {
                next.archived_context.push(ArchivedGroup {
                    label: group.label.clone(),
                    items: vec![item.clone()],
                });
            }
            if prompt_memory_tokens(&next) <= budget {
                *prompt = next;
            }
        }
    }
}

pub(super) fn summary_batches(analyses: &[DocumentAnalysis]) -> Vec<Vec<&DocumentAnalysis>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_tokens = 0usize;
    for analysis in analyses {
        let compact = CompactAnalysis::from(analysis);
        let raw = serde_json::to_string(&compact).unwrap_or_default();
        let tokens = estimate_tokens(&raw);
        if !current.is_empty() && current_tokens.saturating_add(tokens) > SUMMARY_DELTA_TOKEN_BUDGET
        {
            batches.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        current.push(analysis);
        current_tokens = current_tokens.saturating_add(tokens);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::super::items::ItemFlags;
    use super::*;

    #[test]
    fn summary_batches_keep_every_delta_once() {
        let analyses = (0..100)
            .map(|index| DocumentAnalysis {
                id: format!("id-{index}"),
                status: "done".into(),
                summary: "x".repeat(3_000),
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let batches = summary_batches(&analyses);
        assert!(batches.len() > 1);
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), analyses.len());
        assert_eq!(
            batches
                .iter()
                .flat_map(|batch| batch.iter().map(|analysis| analysis.id.as_str()))
                .collect::<Vec<_>>(),
            analyses
                .iter()
                .map(|analysis| analysis.id.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn compact_student_profile_excludes_contact_details() {
        let profile = serde_json::json!({
            "student_id": "1234",
            "name": "Student",
            "faculty": "Faculty",
            "address": "private address",
            "phone": "private phone"
        });

        let compact = compact_student_profile(&profile);
        assert_eq!(compact["student_id"], "1234");
        assert_eq!(compact["name"], "Student");
        assert!(compact.get("address").is_none());
        assert!(compact.get("phone").is_none());
    }

    #[test]
    fn prompt_course_analysis_bounds_memory_without_dropping_priority_context() {
        let mut archive = Vec::new();
        for group_index in 0..20 {
            archive.push(ArchivedGroup {
                label: format!("過去グループ{group_index}"),
                items: (0..5)
                    .map(|item_index| {
                        format!(
                            "過去項目{group_index}-{item_index}: {}",
                            "長い履歴".repeat(20)
                        )
                    })
                    .collect(),
            });
        }
        let analysis = AgentCourseAnalysis {
            summary: "今見るべき結論".into(),
            findings: vec![
                Item {
                    text: "通常の確認事項".into(),
                    ..Default::default()
                },
                Item {
                    text: "今日中に提出する".into(),
                    flags: ItemFlags {
                        action: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            standing_context: vec![Item {
                text: "座席表は新掲示と前回座席を照合する".into(),
                flags: ItemFlags {
                    urgent: true,
                    ..Default::default()
                },
                ..Default::default()
            }],
            archived_context: archive,
            seat: super::super::SeatConclusion {
                assignment: "7列目".into(),
                evidence: (0..20)
                    .map(|index| format!("座席根拠{index}: {}", "根拠".repeat(20)))
                    .collect(),
                confidence: 0.8,
                ..Default::default()
            },
            print_candidates: (0..20)
                .map(|index| PrintCandidate {
                    filename: format!("print-{index}.pdf"),
                    reason: "記入欄あり".repeat(20),
                    confidence: if index == 19 { 0.99 } else { 0.7 },
                    category: "ワークシート".into(),
                })
                .collect(),
            ..Default::default()
        };

        let prompt = prompt_course_analysis_with_budget(&analysis, 900);

        assert!(prompt_memory_tokens(&prompt) <= 900);
        assert_eq!(prompt.summary, "今見るべき結論");
        assert_eq!(prompt.seat.assignment, "7列目");
        assert!(prompt.findings.iter().any(|item| item.flags.action));
        assert!(prompt.standing_context.iter().any(|item| item.flags.urgent));
        assert!(prompt.seat.evidence.len() < analysis.seat.evidence.len());
        assert!(prompt
            .print_candidates
            .iter()
            .any(|candidate| candidate.filename == "print-19.pdf"));
        assert!(prompt.print_candidates.len() < analysis.print_candidates.len());
        assert!(
            prompt
                .archived_context
                .iter()
                .map(|group| group.items.len())
                .sum::<usize>()
                < analysis
                    .archived_context
                    .iter()
                    .map(|group| group.items.len())
                    .sum::<usize>()
        );
        assert_eq!(analysis.archived_context.len(), 20);
    }
}
