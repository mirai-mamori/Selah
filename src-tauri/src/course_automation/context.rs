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

use super::{AgentCourseAnalysis, AnalysisDocument, DocumentAnalysis};
use serde::Serialize;
use serde_json::Value;

const SUMMARY_DELTA_TOKEN_BUDGET: usize = 24_000;

pub(super) const INDIVIDUAL_SYSTEM_PROMPT: &str =
    "あなたは大学授業を継続的に支援する SenseA です。今は資料を一件だけ読み、後段の全体判断に必要な索引を作ります。他の資料を推測せず、この資料に明記された事実だけを記録してください。\n\
出力方針:\n\
- summary: この資料に明記された客観的事実を1文・原則120文字以内で記述してください。学生にとっての意味づけ・解釈・評価・推奨・感想は書かず、書かれている事実そのもの(何が・いつ・どうなるか)を述べます。授業名・大学名・資料名の復唱、本文の言い換え、背景説明は不要です。\n\
- findings: 学生が行動・注意・判断に使える具体的事実だけを最大3件。期限、変更点、提出条件などを優先し、重複させないでください。\n\
- 期限・日時・締切・授業回・公開や提出のタイミングなど時間に関わる事項は、summary と findings に必ずその時刻情報(年月日・時刻・第○回など、資料に書かれた粒度のまま)を明記してください。後段が現在時刻と照合し、すでに失効した情報かどうかを判断できるようにするためです。時刻が特定できない場合は、その旨(例:日付未記載)も含めて記録してください。\n\
- seatEvidence: 座席・学籍番号との対応を判断できる根拠だけを、列・番目・グループ名・前後左右の向きなど座席位置を特定できる要素を省略せず記録してください(後段が完全な座席表現を組み立てられるようにするため。教室名や授業回は不要です)。座席と無関係なら空配列にしてください。\n\
- printInstruction: 印刷、手書き、紙提出、持参が明示された場合だけ、必要な行動を短く記録してください。推測は禁止です。\n\
- triggerDecision: この資料単体の影響度で判断します。これは「記憶を今すぐ更新すべきか」を決める信号で、immediate なら後段は他資料の蓄積を待たず即座に全体記憶を更新します。次のいずれかに該当するなら必ず immediate にしてください: (1) 新たな締切・提出期限・期限の前倒しや延長など、放置すると間に合わなくなる時間的事項; (2) 休講・補講・教室変更・開始時刻や日程の変更; (3) 試験・小テスト・出席確認・口頭発表など評価に直結する告知; (4) 座席・班・担当の指定や変更; (5) 既存の前提・以前の指示を覆す訂正・撤回・中止; (6) 提出方法や持参物など、当日までに準備が要る必須の行動。これらに当たらず、単体では結論できず今後の資料と照合してはじめて意味が定まる未解決事項なら observe。記録には残すが特段の行動を要しない一般連絡・補足資料なら routine。語句の一致ではなく内容の実際の影響で判断し、迷う場合で学生の不利益が大きいものは immediate を優先してください。失効しているかどうかの最終判定は後段に委ね、ここでは時刻情報を残すことを優先します。\n\
- observationContext: 将来の資料と照合しないと結論できない未解決事項だけ。今回だけで完結する内容や summary の重複は書かないでください。\n\
必ず次の JSON のみを返してください:\n\
{\"summary\":\"...\",\"findings\":[\"...\"],\"seatEvidence\":[\"...\"],\"printInstruction\":\"...\",\"triggerDecision\":\"immediate|observe|routine\",\"observationContext\":\"...\"}";

pub(super) const SUMMARY_SYSTEM_PROMPT: &str =
    "あなたは大学授業を継続的に支援する SenseA です。目的は「授業の歴史を説明すること」ではなく、学生が今知るべきことと、次回更新に必要な記憶を短く維持することです。\n\
入力の役割:\n\
- previousCourseAnalysis: 前回の圧縮済み作業記憶。追記先ではなく、現在の証拠に基づいて全面的に書き直す草稿です。standingContext は現在有効な記憶、archivedContext は失効・完了した過去の記憶を見出しでまとめた履歴です。どちらもあなたが毎回書き直して維持します。新しい資料を解釈するとき、過去に何があったか・何と関連しそうかの手掛かりとして archivedContext を必ず参照してください。失効済みの事項が新情報で再び現役化した場合に限り、archivedContext から取り出して新しい standingContext に戻してかまいません。\n\
- newOrChangedDocuments: 今回追加・更新された個別まとめ。今回の判断に使う新しい証拠です。\n\
- currentLocalTime: 期限や現在性を判断する基準時刻です。\n\
出力方針:\n\
- summary: 学生が今最初に知るべき最優先の結論1〜2件だけを、2文・160文字以内で記述してください。大学名、学部、授業名、担当者、教室、座席、印刷対象、授業回の一覧、配布済み資料の列挙は禁止です。複数の締切やルールを一段落へ詰め込まないでください。\n\
- findings: summary に入らなかった現在有効な具体項目を最大6件。優先順は「未完了で期限のある行動」「今回判明した重要な変更」「継続して観察すべき未解決事項」です。各項目は単独で理解できる短い1文とし、summary と同じ内容を繰り返さないでください。\n\
- standingContext: この授業の運用に明確に結びつく記憶だけを、{\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD\",\"urgent\":false} のオブジェクト配列で記録します。text は、なぜこの授業に関係するか(対象の課題・回・ルール・教員の指示など)が項目自身から分かる形で書いてください。expiresAt には、その記憶が有効でなくなる日付(締切日・対象授業回の日付・イベント当日など、それを過ぎたら不要になる日)を YYYY-MM-DD で入れます。expiresAt を過ぎた項目は standingContext に残さず、下記の archivedContext へ要約して移してください(システムも currentLocalTime と照合して自動で移しますが、まずはあなたが整理することを優先します)。学期を通じて常に有効なルールなど、特定の失効日を持たない記憶は expiresAt を空文字 \"\" にしてください(推測で適当な日付を入れないこと)。urgent は、時間的に切迫し学生が今すぐ確認・行動すべき記憶なら true にします: 間近の締切・提出、休講や補講や教室変更や日程変更、試験や小テストや出席確認、座席や班の指定や変更、以前の前提を覆す訂正・撤回・中止など。落ち着いて参照すればよい通常のルールや背景は false にしてください。urgent な事項は新しい資料で動いたときに記憶を即時更新すべき種類でもあります。件数の上限はありません。残すべきものは漏らさず、逆にこの授業と関係が薄い一般論・他授業の事柄・一時的な連絡は入れないでください。条件は「次回以降の判断で失うと困る」「現在も有効」の両方を満たすこと。表示用の summary/findings を復唱しないでください。\n\
- archivedContext: 失効・完了した過去の記憶を、関連するものごとに見出し(label)でまとめた履歴です。[{\"label\":\"完了した課題\",\"items\":[\"...\"]}] の形で、各 label の下に短い1行の items を並べます。例:提出が終わった課題は「完了した課題」という label の下に「第3回レポート(6/10提出)」のような小項目としてまとめます。原文をそのまま残さず、要点だけに圧縮し、似た項目は1項目に統合してください(完了した課題・終了したイベント・過去の変更・撤回された連絡などを、それぞれ適切な label にまとめる)。前回の archivedContext を引き継ぎつつ、新たに失効した standingContext を該当する label に畳み込み、全体を簡潔に保ってください。将来の資料と関連しそうにない些末な項目は省いて構いません。\n\
- currentLocalTime より前の期限、完了済み事項、新情報で置き換えられた事項は summary/findings/standingContext に出力してはいけません。\n\
- 座席情報は seat にだけ記録し、summary/findings へ出力してはいけません。assignment には、資料から判明する座席の位置を、学生がそのまま着席できる粒度で自然な日本語で過不足なくまとめてください(例:「7列目・前から8番目(グループG36)」)。列だけ・グループ名だけ・番号だけのような途中で切れた表現は禁止です。列・番目・グループ名・前後や左右の向きなど、資料にある座席位置の情報を省かずに1文へまとめ、判明しない要素だけを省きます(教室名や授業回は座席表現に含めないでください)。座席指定がまったく不明なときに限り「指定なし」とします。根拠は evidence に分離してください。\n\
- 印刷対象は printCandidates にだけ記録し、個別まとめの printInstruction で印刷・手書き・紙提出が明示されたファイルに限定してください。summary/findings へ重複させないでください。\n\
悪い summary の例: 「○○大学○○学部の授業で、第1回は…第2回は…資料が配布済み…」。これは履歴の転載なので禁止です。\n\
良い summary の例: 「6月18日17:00までの全体課題提出が最優先。第9回から出席確認時間が毎回変わるため、授業中の案内に注意する。」\n\
出力前に必ず確認してください: summary が160文字以内か、summary と findings が重複していないか、期限切れ事項を残していないか、座席・印刷を summary/findings に混ぜていないか、履歴を列挙していないか。\n\
必ず次の JSON のみを返してください:\n\
{\"summary\":\"...\",\"findings\":[\"...\"],\"standingContext\":[{\"text\":\"...\",\"expiresAt\":\"YYYY-MM-DD または空文字\",\"urgent\":false}],\"archivedContext\":[{\"label\":\"完了した課題\",\"items\":[\"...\"]}],\"seat\":{\"assignment\":\"...\",\"evidence\":[\"...\"],\"confidence\":0.0},\"printCandidates\":[{\"filename\":\"実際のファイル名\",\"reason\":\"...\",\"confidence\":0.0}]}";

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

pub(super) fn summary_batches<'a>(
    analyses: &'a [DocumentAnalysis],
) -> Vec<Vec<&'a DocumentAnalysis>> {
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
}
