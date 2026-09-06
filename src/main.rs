use std::collections::HashSet;

use regex::Regex;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

struct Handler {
    twitter_re: Regex,
    allowed_channels: HashSet<u64>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // bot自身のメッセージは無視
        if msg.author.bot {
            return;
        }

        // 対象チャンネル以外は無視
        if !self.allowed_channels.contains(&msg.channel_id.get()) {
            return;
        }

        let content = &msg.content;

        // twitter.com / x.com を含むか確認
        if !self.twitter_re.is_match(content) {
            return;
        }

        // URLをfxtwitter.comに置換
        let new_content = self.twitter_re.replace_all(content, "https://fxtwitter.com$2");

        // 元メッセージを削除
        if let Err(e) = msg.delete(&ctx.http).await {
            eprintln!("メッセージ削除失敗: {:?}", e);
            return;
        }

        // 投稿者名 + 置換後メッセージを送信
        let reply = format!("**{}**: {}", msg.author.name, new_content);
        if let Err(e) = msg.channel_id.say(&ctx.http, reply).await {
            eprintln!("メッセージ送信失敗: {:?}", e);
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} がオンラインになりました！", ready.user.name);
    }
}

/// カンマ区切りのチャンネルID文字列を解析する。
/// 空白は無視し、空の要素は読み飛ばす。
fn parse_channel_ids(raw: &str) -> Result<HashSet<u64>, String> {
    let mut ids = HashSet::new();

    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let id = part
            .parse::<u64>()
            .map_err(|_| format!("チャンネルIDとして解釈できません: {part}"))?;
        ids.insert(id);
    }

    if ids.is_empty() {
        return Err("チャンネルIDが1つも指定されていません".to_string());
    }

    Ok(ids)
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKENが設定されていません");

    let raw_channels =
        std::env::var("ALLOWED_CHANNEL_IDS").expect("ALLOWED_CHANNEL_IDSが設定されていません");
    let allowed_channels = parse_channel_ids(&raw_channels)
        .unwrap_or_else(|e| panic!("ALLOWED_CHANNEL_IDSの解析に失敗しました: {e}"));

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler {
        twitter_re: Regex::new(r"https?://(twitter\.com|x\.com)(/\S*)?").unwrap(),
        allowed_channels,
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("クライアント作成失敗");

    if let Err(e) = client.start().await {
        eprintln!("クライアントエラー: {:?}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 単一のidを解析できる() {
        let ids = parse_channel_ids("123").unwrap();
        assert_eq!(ids, HashSet::from([123]));
    }

    #[test]
    fn カンマ区切りと空白を解析できる() {
        let ids = parse_channel_ids(" 123 , 456,789 ").unwrap();
        assert_eq!(ids, HashSet::from([123, 456, 789]));
    }

    #[test]
    fn 末尾のカンマは無視される() {
        let ids = parse_channel_ids("123,456,").unwrap();
        assert_eq!(ids, HashSet::from([123, 456]));
    }

    #[test]
    fn 数値以外はエラーになる() {
        assert!(parse_channel_ids("123,abc").is_err());
    }

    #[test]
    fn 空文字列はエラーになる() {
        assert!(parse_channel_ids("").is_err());
        assert!(parse_channel_ids("  ,  ").is_err());
    }
}