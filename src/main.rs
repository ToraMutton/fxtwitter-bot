use regex::Regex;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

struct Handler {
    twitter_re: Regex,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // bot自身のメッセージは無視
        if msg.author.bot {
            return;
        }

        let allowed_channel: u64 = 1432361849826836600;
        if msg.channel_id != allowed_channel {
            return;
        }

        let content = &msg.content;

        // twitter.com / x.com を含むか確認
        if !self.twitter_re.is_match(content) {
            return;
        }

        // URLをfxtwitter.comに置換
        let new_content = self.twitter_re.replace_all(content, "fxtwitter.com");

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

#[tokio::main]
async fn main() {
    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKENが設定されていません");

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let handler = Handler {
        twitter_re: Regex::new(r"https?://(twitter\.com|x\.com)/").unwrap(),
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(handler)
        .await
        .expect("クライアント作成失敗");

    if let Err(e) = client.start().await {
        eprintln!("クライアントエラー: {:?}", e);
    }
}