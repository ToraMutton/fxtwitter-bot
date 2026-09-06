//! チャンネル履歴から読み取った投稿を集計する。
//!
//! このモジュールは Discord に依存しない純粋なロジックだけを持つ。
//! そのため、実際に Bot を動かさなくてもテストできる。

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

/// 元ツイートへの参照。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TweetRef {
    /// 元ツイートのアカウント名（URLの `/account/status/` 部分）
    pub account: String,
    /// ツイートID
    pub id: String,
}

/// 履歴から読み取った1メッセージ分の共有記録。
#[derive(Debug, Clone, PartialEq)]
pub struct SharedPost {
    /// 共有した人（Discordの表示名）
    pub author: String,
    /// そのメッセージに含まれていたツイート
    pub tweets: Vec<TweetRef>,
    /// メッセージに付いたリアクションの合計数
    pub reactions: u64,
    /// 投稿時刻（UNIX秒）
    pub timestamp: i64,
    /// メッセージID（リンク生成用）
    pub message_id: u64,
}

/// 集計結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Tally {
    /// 共有されたツイートの総数
    pub total_tweets: usize,
    /// 投稿メッセージの総数
    pub total_messages: usize,
    /// 参加した人数
    pub participants: usize,
    /// 投稿数ランキング（多い順）
    pub by_author: Vec<(String, usize)>,
    /// 元アカウントのランキング（多い順）
    pub by_account: Vec<(String, usize)>,
    /// リアクション数の多い投稿（多い順、リアクション0のものは含まない）
    pub top_reacted: Vec<SharedPost>,
}

/// X の URL には `https://x.com/i/status/123` のようにユーザー名を含まない形式がある。
/// これらの予約パスはアカウント名として扱わない。
const RESERVED_PATHS: [&str; 4] = ["i", "web", "intent", "home"];

/// URL から取り出した文字列が、実在しうるアカウント名かどうか。
pub fn is_account_name(candidate: &str) -> bool {
    !RESERVED_PATHS
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(candidate))
}

fn tweet_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https?://(?:www\.)?(?:fxtwitter|fixupx)\.com/([A-Za-z0-9_]{1,15})/status/(\d+)")
            .expect("ツイートURLの正規表現が不正")
    })
}

fn author_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\*\*(.+?)\*\*:").expect("投稿者名の正規表現が不正"))
}

/// メッセージ本文から、含まれている元ツイートをすべて取り出す。
pub fn extract_tweets(content: &str) -> Vec<TweetRef> {
    tweet_url_re()
        .captures_iter(content)
        .map(|c| TweetRef {
            account: c[1].to_string(),
            id: c[2].to_string(),
        })
        .collect()
}

/// Bot が投稿し直したメッセージから、元の投稿者名を取り出す。
pub fn extract_author(content: &str) -> Option<String> {
    author_prefix_re()
        .captures(content)
        .map(|c| c[1].trim().to_string())
        .filter(|name| !name.is_empty())
}

/// 1件のメッセージを集計対象として解釈する。対象外なら `None`。
///
/// Bot の投稿は `**なまえ**: ` 形式のものだけを対象にする。
/// これにより、Bot 自身が投稿したランキング（本文にツイートURLを含みうる）が
/// 次回の集計に混ざることを防いでいる。
pub fn parse_message(
    content: &str,
    author_name: &str,
    is_bot: bool,
) -> Option<(String, Vec<TweetRef>)> {
    let tweets = extract_tweets(content);
    if tweets.is_empty() {
        return None;
    }

    if is_bot {
        let author = extract_author(content)?;
        Some((author, tweets))
    } else {
        // 人間が直接 fxtwitter のURLを貼った場合も拾う
        Some((author_name.to_string(), tweets))
    }
}

/// 件数の多い順に並べる。同数のときは名前順にして結果を安定させる。
fn ranked(counts: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// 投稿の一覧を集計する。
pub fn tally(posts: &[SharedPost]) -> Tally {
    let mut author_counts: HashMap<String, usize> = HashMap::new();
    let mut account_counts: HashMap<String, usize> = HashMap::new();

    for post in posts {
        *author_counts.entry(post.author.clone()).or_default() += post.tweets.len();
        for tweet in &post.tweets {
            // ユーザー名を含まない形式のURLは、元アカウントを特定できないため数えない
            if is_account_name(&tweet.account) {
                *account_counts.entry(tweet.account.clone()).or_default() += 1;
            }
        }
    }

    let mut top_reacted: Vec<SharedPost> =
        posts.iter().filter(|p| p.reactions > 0).cloned().collect();
    // リアクションが多い順。同数なら新しい投稿を先に。
    top_reacted.sort_by(|a, b| {
        b.reactions
            .cmp(&a.reactions)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    Tally {
        total_tweets: posts.iter().map(|p| p.tweets.len()).sum(),
        total_messages: posts.len(),
        participants: author_counts.len(),
        by_author: ranked(author_counts),
        by_account: ranked(account_counts),
        top_reacted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(author: &str, accounts: &[&str], reactions: u64, timestamp: i64) -> SharedPost {
        SharedPost {
            author: author.to_string(),
            tweets: accounts
                .iter()
                .enumerate()
                .map(|(i, a)| TweetRef {
                    account: a.to_string(),
                    id: format!("{timestamp}{i}"),
                })
                .collect(),
            reactions,
            timestamp,
            message_id: timestamp as u64,
        }
    }

    #[test]
    fn ツイートurlを取り出せる() {
        let tweets = extract_tweets("**someone**: https://fxtwitter.com/jack/status/20 みて");
        assert_eq!(
            tweets,
            vec![TweetRef {
                account: "jack".to_string(),
                id: "20".to_string()
            }]
        );
    }

    #[test]
    fn 複数のurlを取り出せる() {
        let tweets = extract_tweets(
            "https://fxtwitter.com/a/status/1 と https://fixupx.com/b/status/2 だよ",
        );
        assert_eq!(tweets.len(), 2);
        assert_eq!(tweets[1].account, "b");
    }

    #[test]
    fn 関係ないurlは拾わない() {
        assert!(extract_tweets("https://example.com/jack/status/20").is_empty());
        assert!(extract_tweets("https://fxtwitter.com/jack").is_empty());
    }

    #[test]
    fn 投稿者名を取り出せる() {
        assert_eq!(
            extract_author("**なまえ**: https://fxtwitter.com/a/status/1"),
            Some("なまえ".to_string())
        );
        assert_eq!(extract_author("ただの文章"), None);
    }

    #[test]
    fn botの投稿は投稿者名がある場合だけ集計対象になる() {
        // Bot が貼り直したメッセージ
        let parsed = parse_message(
            "**ゆーざー**: https://fxtwitter.com/a/status/1",
            "fxtwitter-bot",
            true,
        );
        assert_eq!(parsed.unwrap().0, "ゆーざー");

        // Bot 自身のランキング投稿（投稿者名の接頭辞がない）は対象外
        let parsed = parse_message(
            "今週の最強ツイート https://fxtwitter.com/a/status/1",
            "fxtwitter-bot",
            true,
        );
        assert!(parsed.is_none());
    }

    #[test]
    fn 人間が直接貼った場合は本人名義になる() {
        let parsed = parse_message("https://fxtwitter.com/a/status/1", "ちょくせつ", false);
        assert_eq!(parsed.unwrap().0, "ちょくせつ");
    }

    #[test]
    fn ツイートを含まないメッセージは対象外() {
        assert!(parse_message("おはよう", "だれか", false).is_none());
    }

    #[test]
    fn 投稿数を集計できる() {
        let posts = vec![
            post("A", &["cat"], 0, 100),
            post("A", &["dog"], 0, 200),
            post("B", &["cat", "bird"], 0, 300),
        ];
        let t = tally(&posts);

        assert_eq!(t.total_tweets, 4);
        assert_eq!(t.total_messages, 3);
        assert_eq!(t.participants, 2);
        assert_eq!(t.by_author, vec![("A".into(), 2), ("B".into(), 2)]);
        assert_eq!(
            t.by_account,
            vec![("cat".into(), 2), ("bird".into(), 1), ("dog".into(), 1)]
        );
    }

    #[test]
    fn ユーザー名を含まないurlはアカウントとして数えない() {
        // https://x.com/i/status/123 のような形式
        let posts = vec![
            post("A", &["i"], 0, 100),
            post("B", &["web"], 0, 200),
            post("C", &["cat"], 0, 300),
        ];
        let t = tally(&posts);

        // 投稿数としては3件すべて数える
        assert_eq!(t.total_tweets, 3);
        // アカウントランキングには実在しうる名前だけ載る
        assert_eq!(t.by_account, vec![("cat".into(), 1)]);
    }

    #[test]
    fn 予約パスの判定は大文字小文字を区別しない() {
        assert!(!is_account_name("i"));
        assert!(!is_account_name("I"));
        assert!(is_account_name("ice"));
        assert!(is_account_name("cat_movie"));
    }

    #[test]
    fn リアクション順に並ぶ() {
        let posts = vec![
            post("A", &["x"], 1, 100),
            post("B", &["y"], 5, 200),
            post("C", &["z"], 0, 300),
        ];
        let t = tally(&posts);

        // リアクション0の投稿は除外される
        assert_eq!(t.top_reacted.len(), 2);
        assert_eq!(t.top_reacted[0].author, "B");
        assert_eq!(t.top_reacted[1].author, "A");
    }

    #[test]
    fn 空でも落ちない() {
        let t = tally(&[]);
        assert_eq!(t.total_tweets, 0);
        assert_eq!(t.participants, 0);
        assert!(t.by_author.is_empty());
        assert!(t.top_reacted.is_empty());
    }
}
