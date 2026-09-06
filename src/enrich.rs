//! FxTwitter の公開 API から元ツイートの情報を取得する。
//!
//! 相手は有志が無料で運営しているサービスなので、次の点に配慮している。
//!
//! - 同時接続数を絞る
//! - 一度取得した結果はプロセス内にキャッシュして再問い合わせしない
//! - 1回の集計で投げる件数に上限を設ける
//! - 取得できなくても集計本体は成立させる（この情報は「あれば嬉しい」扱い）

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tokio::sync::{Mutex, Semaphore};

const API_BASE: &str = "https://api.fxtwitter.com/status/";
const TIMEOUT: Duration = Duration::from_secs(10);

/// 同時に投げるリクエスト数。
const CONCURRENCY: usize = 4;

/// 1回の集計で問い合わせる件数の上限。
pub const MAX_REQUESTS: usize = 300;

/// 元ツイートから取得できた情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TweetInfo {
    pub id: String,
    /// 実際の投稿者（URLに含まれない場合でもここで判明する）
    pub screen_name: String,
    pub text: String,
    pub likes: u64,
    pub url: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    tweet: Option<ApiTweet>,
}

#[derive(Deserialize)]
struct ApiTweet {
    id: String,
    url: String,
    text: String,
    likes: u64,
    author: ApiAuthor,
}

#[derive(Deserialize)]
struct ApiAuthor {
    screen_name: String,
}

/// 問い合わせ結果。削除済みツイートを繰り返し問い合わせないため、
/// 「存在しなかった」ことも記録する。
#[derive(Clone)]
enum Cached {
    Found(Box<TweetInfo>),
    NotFound,
}

pub struct Enricher {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, Cached>>,
}

impl Enricher {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!(
                "fxtwitter-bot/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/ToraMutton/fxtwitter-bot)"
            ))
            .build()?;

        Ok(Self {
            client,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// 複数のツイートIDについて情報を取得する。
    ///
    /// 取得できなかったものは結果に含まれない。呼び出し側は
    /// 「一部しか取れない」ことを前提に扱うこと。
    pub async fn fetch_many(&self, ids: &[String]) -> HashMap<String, TweetInfo> {
        let mut found = HashMap::new();
        let mut missing = Vec::new();

        // まずキャッシュを引く
        {
            let cache = self.cache.lock().await;
            let mut seen = std::collections::HashSet::new();
            for id in ids {
                if !seen.insert(id) {
                    continue;
                }
                match cache.get(id) {
                    Some(Cached::Found(info)) => {
                        found.insert(id.clone(), (**info).clone());
                    }
                    Some(Cached::NotFound) => {}
                    None => missing.push(id.clone()),
                }
            }
        }

        missing.truncate(MAX_REQUESTS);
        if missing.is_empty() {
            return found;
        }

        let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
        let mut tasks = tokio::task::JoinSet::new();
        for id in missing {
            let client = self.client.clone();
            let semaphore = Arc::clone(&semaphore);
            tasks.spawn(async move {
                // 同時接続数を超えないよう順番待ちする
                let _permit = semaphore.acquire().await;
                let outcome = fetch_one(&client, &id).await;
                (id, outcome)
            });
        }

        let mut fresh = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            let Ok((id, outcome)) = joined else {
                continue;
            };
            match outcome {
                Ok(Some(info)) => {
                    found.insert(id.clone(), info.clone());
                    fresh.push((id, Cached::Found(Box::new(info))));
                }
                Ok(None) => fresh.push((id, Cached::NotFound)),
                // 通信エラーは一時的なものかもしれないのでキャッシュしない
                Err(e) => eprintln!("ツイート {} の取得に失敗: {}", id, e),
            }
        }

        if !fresh.is_empty() {
            let mut cache = self.cache.lock().await;
            cache.extend(fresh);
        }

        found
    }
}

/// 1件のツイートを問い合わせる。存在しなかった場合は `Ok(None)`。
async fn fetch_one(
    client: &reqwest::Client,
    id: &str,
) -> Result<Option<TweetInfo>, reqwest::Error> {
    let response = client.get(format!("{API_BASE}{id}")).send().await?;

    if !response.status().is_success() {
        // 削除済み・非公開などはエラーではなく「取得できないもの」として扱う
        return Ok(None);
    }

    let body: ApiResponse = response.json().await?;
    Ok(body.tweet.map(|t| TweetInfo {
        id: t.id,
        screen_name: t.author.screen_name,
        text: t.text,
        likes: t.likes,
        url: t.url,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際の API を叩いて、応答の形が想定どおりか確かめる。
    ///
    /// 外部サービスに依存するので通常のテストからは外してある。
    /// 実行するには `cargo test -- --ignored`。
    #[tokio::test]
    #[ignore = "外部APIに接続するため"]
    async fn 実在するツイートを取得できる() {
        let enricher = Enricher::new().unwrap();
        // Twitter 史上最初のツイート
        let result = enricher.fetch_many(&["20".to_string()]).await;

        let info = result.get("20").expect("取得できること");
        assert_eq!(info.screen_name, "jack");
        assert_eq!(info.text, "just setting up my twttr");
        assert!(info.likes > 0, "いいね数が取れていること");
        assert!(info.url.contains("/status/20"));
    }

    #[tokio::test]
    #[ignore = "外部APIに接続するため"]
    async fn 存在しないツイートは結果に含まれない() {
        let enricher = Enricher::new().unwrap();
        let result = enricher.fetch_many(&["1".to_string()]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    #[ignore = "外部APIに接続するため"]
    async fn 二度目はキャッシュから返る() {
        let enricher = Enricher::new().unwrap();
        let ids = vec!["20".to_string()];

        enricher.fetch_many(&ids).await;
        // 1件だけキャッシュされているはず
        assert_eq!(enricher.cache.lock().await.len(), 1);

        let again = enricher.fetch_many(&ids).await;
        assert_eq!(again.get("20").unwrap().screen_name, "jack");
    }
}
