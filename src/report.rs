//! 集計結果を Discord へ投稿する文章に整形する。
//!
//! ここも Discord に依存しない純粋な文字列処理なのでテストできる。

use crate::tally::Tally;

const MEDALS: [&str; 3] = ["🥇", "🥈", "🥉"];

/// 集計対象の期間。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// 直近7日間
    Week,
    /// 直近30日間
    Month,
    /// 全期間
    All,
}

impl Period {
    pub fn label(&self) -> &'static str {
        match self {
            Period::Week => "直近7日間",
            Period::Month => "直近30日間",
            Period::All => "全期間",
        }
    }

    /// 前の期間と比べる表現。全期間には比較対象がない。
    pub fn comparison_label(&self) -> Option<&'static str> {
        match self {
            Period::Week => Some("前週比"),
            Period::Month => Some("前月比"),
            Period::All => None,
        }
    }
}

/// Discord のメッセージへ飛ぶリンクを組み立てる。
fn message_link(guild_id: u64, channel_id: u64, message_id: u64) -> String {
    format!("https://discord.com/channels/{guild_id}/{channel_id}/{message_id}")
}

/// Discord のメンションや装飾として解釈されうる文字を無効化する。
///
/// 表示名は利用者が自由に決められるため、そのまま埋め込むと
/// ランキングの体裁が崩れたり、意図しないメンションが飛んだりする。
fn escape(name: &str) -> String {
    name.replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace('~', "\\~")
        .replace('|', "\\|")
        .replace('@', "@\u{200b}")
}

/// 増減を「+3」「-2」「±0」の形にする。
fn diff_text(current: usize, previous: usize) -> String {
    let cur = current as i64;
    let prev = previous as i64;
    match cur - prev {
        0 => "±0".to_string(),
        d if d > 0 => format!("+{d}"),
        d => format!("{d}"),
    }
}

/// 集計結果を投稿用の本文にする。
pub fn format_report(
    tally: &Tally,
    period: Period,
    guild_id: u64,
    channel_id: u64,
    previous_total: Option<usize>,
) -> String {
    if tally.total_tweets == 0 {
        return format!("**{}** の投稿はありませんでした。", period.label());
    }

    let mut out = format!("# 🏆 {} のランキング\n\n", period.label());

    // ── 全体統計 ──
    out.push_str("## 📊 全体\n");
    out.push_str(&format!(
        "投稿 **{}件** ／ 参加 **{}人**",
        tally.total_tweets, tally.participants
    ));
    if let (Some(prev), Some(label)) = (previous_total, period.comparison_label()) {
        out.push_str(&format!(
            " ／ {} **{}件**",
            label,
            diff_text(tally.total_tweets, prev)
        ));
    }
    out.push_str("\n\n");

    // ── 投稿数ランキング ──
    out.push_str("## 👑 投稿数ランキング\n");
    for (i, (author, count)) in tally.by_author.iter().take(3).enumerate() {
        out.push_str(&format!(
            "{} {} — {}件\n",
            MEDALS[i],
            escape(author),
            count
        ));
    }
    out.push('\n');

    // ── リアクションランキング ──
    if !tally.top_reacted.is_empty() {
        out.push_str("## 🔥 リアクションが多かった投稿\n");
        for (i, post) in tally.top_reacted.iter().take(3).enumerate() {
            out.push_str(&format!(
                "{} [{}]({}) — {} 個\n",
                MEDALS[i],
                escape(&post.author),
                message_link(guild_id, channel_id, post.message_id),
                post.reactions
            ));
        }
        out.push('\n');
    }

    // ── 元アカウント ──
    out.push_str("## 🐦 よく貼られたアカウント\n");
    for (account, count) in tally.by_account.iter().take(5) {
        out.push_str(&format!("- `@{account}` — {count}件\n"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tally::{tally, SharedPost, TweetRef};

    fn post(author: &str, account: &str, reactions: u64, id: u64) -> SharedPost {
        SharedPost {
            author: author.to_string(),
            tweets: vec![TweetRef {
                account: account.to_string(),
                id: id.to_string(),
            }],
            reactions,
            timestamp: id as i64,
            message_id: id,
        }
    }

    fn sample() -> Tally {
        tally(&[
            post("あきら", "cat_movie", 3, 1),
            post("あきら", "cat_movie", 0, 2),
            post("ばなな", "dog_clip", 7, 3),
        ])
    }

    #[test]
    fn 投稿がなければその旨を返す() {
        let t = tally(&[]);
        let text = format_report(&t, Period::Week, 1, 2, None);
        assert!(text.contains("投稿はありませんでした"));
    }

    #[test]
    fn 主要な項目が含まれる() {
        let text = format_report(&sample(), Period::Week, 111, 222, None);

        assert!(text.contains("投稿 **3件**"));
        assert!(text.contains("参加 **2人**"));
        assert!(text.contains("🥇 あきら — 2件"));
        assert!(text.contains("🥈 ばなな — 1件"));
        assert!(text.contains("`@cat_movie` — 2件"));
        // リアクション最多が先頭に来る
        assert!(text.contains("🥇 [ばなな](https://discord.com/channels/111/222/3) — 7 個"));
    }

    #[test]
    fn 前期比が表示される() {
        let text = format_report(&sample(), Period::Week, 1, 2, Some(1));
        assert!(text.contains("前週比 **+2件**"));

        let text = format_report(&sample(), Period::Week, 1, 2, Some(3));
        assert!(text.contains("前週比 **±0件**"));

        let text = format_report(&sample(), Period::Week, 1, 2, Some(10));
        assert!(text.contains("前週比 **-7件**"));
    }

    #[test]
    fn 全期間では前期比を出さない() {
        let text = format_report(&sample(), Period::All, 1, 2, Some(1));
        assert!(!text.contains("比"));
    }

    #[test]
    fn 表示名の装飾文字を無効化する() {
        let t = tally(&[post("**ボス**", "acc", 0, 1)]);
        let text = format_report(&t, Period::Week, 1, 2, None);
        // そのまま出ると太字として解釈されてしまう
        assert!(text.contains("\\*\\*ボス\\*\\*"));
    }

    #[test]
    fn メンションが飛ばないようにする() {
        let t = tally(&[post("@everyone", "acc", 0, 1)]);
        let text = format_report(&t, Period::Week, 1, 2, None);
        assert!(!text.contains("@everyone"));
    }

    #[test]
    fn リアクションがなければその節を省く() {
        let t = tally(&[post("だれか", "acc", 0, 1)]);
        let text = format_report(&t, Period::Week, 1, 2, None);
        assert!(!text.contains("リアクション"));
    }
}
