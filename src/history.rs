//! Discord のチャンネル履歴を遡って、集計対象の投稿を集める。
//!
//! データベースを持たず、Discord の履歴そのものを台帳として扱う。

use serenity::builder::GetMessages;
use serenity::http::Http;
use serenity::model::id::ChannelId;

use crate::tally::{parse_message, SharedPost};

/// 一度に取得できる件数（Discord API の上限）
const BATCH: u8 = 100;

/// 遡る件数の上限。履歴が非常に長い場合に走り続けないための安全弁。
const MAX_MESSAGES: usize = 10_000;

/// `since`（UNIX秒）以降のメッセージを遡り、集計対象の投稿を集める。
///
/// 新しい方から取得し、`since` より古いメッセージに到達した時点で打ち切る。
/// `since` に 0 を渡すと全期間を対象にする。
pub async fn collect_posts(
    http: &Http,
    channel_id: ChannelId,
    since: i64,
) -> Result<Vec<SharedPost>, serenity::Error> {
    let mut posts = Vec::new();
    let mut before = None;
    let mut scanned = 0usize;

    loop {
        let mut request = GetMessages::new().limit(BATCH);
        if let Some(id) = before {
            request = request.before(id);
        }

        let batch = channel_id.messages(http, request).await?;
        if batch.is_empty() {
            break;
        }

        let mut reached_end = false;
        for msg in &batch {
            let timestamp = msg.timestamp.unix_timestamp();
            if timestamp < since {
                reached_end = true;
                break;
            }

            if let Some((author, tweets)) =
                parse_message(&msg.content, &msg.author.name, msg.author.bot)
            {
                posts.push(SharedPost {
                    author,
                    tweets,
                    timestamp,
                    message_id: msg.id.get(),
                });
            }
        }

        scanned += batch.len();
        before = batch.last().map(|m| m.id);

        if reached_end || batch.len() < BATCH as usize || scanned >= MAX_MESSAGES {
            break;
        }
    }

    Ok(posts)
}
