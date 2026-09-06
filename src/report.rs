//! 集計結果を Discord へ投稿する文章に整形する。
//!
//! ここも Discord に依存しない純粋な文字列処理なのでテストできる。

use crate::enrich::TweetInfo;
use crate::tally::Tally;

const MEDALS: [&str; 3] = ["🥇", "🥈", "🥉"];

/// ツイート本文を1行で見せるときの長さ
const SNIPPET_LEN: usize = 80;

/// FxTwitter API から得られた「あれば載せる」情報。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Highlights {
    /// いいね数が最も多かったツイート
    pub top_tweet: Option<TweetInfo>,
    /// ハッシュタグの出現数（多い順）
    pub hashtags: Vec<(String, usize)>,
}

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

/// ツイート本文を1行に収まる長さへ整える。
fn snippet(text: &str) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() <= SNIPPET_LEN {
        return single_line;
    }
    let mut out: String = single_line.chars().take(SNIPPET_LEN).collect();
    out.push('…');
    out
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
    previous_total: Option<usize>,
    highlights: &Highlights,
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

    // ── 最強ツイート ──
    if let Some(tweet) = &highlights.top_tweet {
        out.push_str("## 💥 最強ツイート\n");
        out.push_str(&format!(
            "[`@{}`]({}) — ❤️ {}\n",
            tweet.screen_name,
            tweet.url,
            format_count(tweet.likes)
        ));
        let text = snippet(&tweet.text);
        if !text.is_empty() {
            out.push_str(&format!("> {}\n", escape(&text)));
        }
        out.push('\n');
    }

    // ── 元アカウント ──
    out.push_str("## 🐦 よく貼られたアカウント\n");
    for (account, count) in tally.by_account.iter().take(5) {
        out.push_str(&format!("- `@{account}` — {count}件\n"));
    }

    // ── ハッシュタグ ──
    if !highlights.hashtags.is_empty() {
        out.push_str("\n## 🔤 よく出たハッシュタグ\n");
        let tags: Vec<String> = highlights
            .hashtags
            .iter()
            .take(5)
            .map(|(tag, count)| format!("`#{tag}` ({count})"))
            .collect();
        out.push_str(&tags.join(" 　"));
        out.push('\n');
    }

    out
}

/// いいね数を読みやすくする（12345 → 1.2万）。
fn format_count(count: u64) -> String {
    if count < 10_000 {
        return count.to_string();
    }
    format!("{:.1}万", count as f64 / 10_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tally::{tally, SharedPost, TweetRef};

    fn post(author: &str, account: &str, id: u64) -> SharedPost {
        SharedPost {
            author: author.to_string(),
            tweets: vec![TweetRef {
                account: account.to_string(),
                id: id.to_string(),
            }],
            timestamp: id as i64,
            message_id: id,
        }
    }

    fn sample() -> Tally {
        tally(&[
            post("あきら", "cat_movie", 1),
            post("あきら", "cat_movie", 2),
            post("ばなな", "dog_clip", 3),
        ])
    }

    fn none() -> Highlights {
        Highlights::default()
    }

    #[test]
    fn 投稿がなければその旨を返す() {
        let t = tally(&[]);
        let text = format_report(&t, Period::Week, None, &none());
        assert!(text.contains("投稿はありませんでした"));
    }

    #[test]
    fn 主要な項目が含まれる() {
        let text = format_report(&sample(), Period::Week, None, &none());

        assert!(text.contains("投稿 **3件**"));
        assert!(text.contains("参加 **2人**"));
        assert!(text.contains("🥇 あきら — 2件"));
        assert!(text.contains("🥈 ばなな — 1件"));
        assert!(text.contains("`@cat_movie` — 2件"));
    }

    #[test]
    fn 前期比が表示される() {
        let text = format_report(&sample(), Period::Week, Some(1), &none());
        assert!(text.contains("前週比 **+2件**"));

        let text = format_report(&sample(), Period::Week, Some(3), &none());
        assert!(text.contains("前週比 **±0件**"));

        let text = format_report(&sample(), Period::Week, Some(10), &none());
        assert!(text.contains("前週比 **-7件**"));
    }

    #[test]
    fn 全期間では前期比を出さない() {
        let text = format_report(&sample(), Period::All, Some(1), &none());
        assert!(!text.contains("比"));
    }

    #[test]
    fn 表示名の装飾文字を無効化する() {
        let t = tally(&[post("**ボス**", "acc", 1)]);
        let text = format_report(&t, Period::Week, None, &none());
        // そのまま出ると太字として解釈されてしまう
        assert!(text.contains("\\*\\*ボス\\*\\*"));
    }

    #[test]
    fn メンションが飛ばないようにする() {
        let t = tally(&[post("@everyone", "acc", 1)]);
        let text = format_report(&t, Period::Week, None, &none());
        assert!(!text.contains("@everyone"));
    }

    #[test]
    fn api情報がなければ該当の節を省く() {
        let text = format_report(&sample(), Period::Week, None, &none());
        assert!(!text.contains("最強ツイート"));
        assert!(!text.contains("ハッシュタグ"));
    }

    #[test]
    fn 最強ツイートとハッシュタグを表示する() {
        let highlights = Highlights {
            top_tweet: Some(TweetInfo {
                id: "1".into(),
                screen_name: "cat_movie".into(),
                text: "かわいい猫\nです".into(),
                likes: 12345,
                url: "https://x.com/cat_movie/status/1".into(),
            }),
            hashtags: vec![("猫".into(), 3), ("犬".into(), 1)],
        };
        let text = format_report(&sample(), Period::Week, None, &highlights);

        assert!(text.contains("## 💥 最強ツイート"));
        assert!(text.contains("[`@cat_movie`](https://x.com/cat_movie/status/1) — ❤️ 1.2万"));
        // 本文の改行は1行にまとめる
        assert!(text.contains("> かわいい猫 です"));
        assert!(text.contains("`#猫` (3)"));
    }

    #[test]
    fn いいね数を読みやすくする() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(9999), "9999");
        assert_eq!(format_count(10_000), "1.0万");
        assert_eq!(format_count(123_456), "12.3万");
    }

    #[test]
    fn 長い本文は切り詰める() {
        let long = "あ".repeat(200);
        let out = snippet(&long);
        assert_eq!(out.chars().count(), SNIPPET_LEN + 1, "省略記号のぶんだけ長い");
        assert!(out.ends_with('…'));
    }
}
