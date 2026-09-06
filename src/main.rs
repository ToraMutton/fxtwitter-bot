mod enrich;
mod history;
mod report;
mod schedule;
mod tally;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use regex::Regex;
use serenity::async_trait;
use serenity::builder::{
    CreateCommand, CreateCommandOption, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
};
use serenity::http::Http;
use serenity::model::application::{CommandOptionType, Interaction};
use serenity::model::channel::{Channel, Message};
use serenity::model::gateway::Ready;
use serenity::model::id::ChannelId;
use serenity::prelude::*;

use enrich::Enricher;
use report::{format_report, Highlights};
use schedule::Span;

/// Discord のメッセージ1件に入れられる文字数の上限には少し余裕を持たせる。
const MAX_CONTENT: usize = 1900;

/// 定期投稿の時刻が来ていないか確認する間隔。
const SCHEDULER_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Bot 全体で共有する状態。
///
/// 定期投稿は別のタスクとして動くため、`Arc` で共有できる形にしてある。
struct Bot {
    allowed_channels: HashSet<u64>,
    enricher: Enricher,
}

struct Handler {
    twitter_re: Regex,
    bot: Arc<Bot>,
    /// 定期投稿タスクを起動済みかどうか。
    /// `ready` は再接続のたびに呼ばれるため、二重起動を防ぐ必要がある。
    scheduler_started: AtomicBool,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        // bot自身のメッセージは無視
        if msg.author.bot {
            return;
        }

        // 対象チャンネル以外は無視
        if !self.bot.allowed_channels.contains(&msg.channel_id.get()) {
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

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
        if command.data.name != "ranking" {
            return;
        }

        if !self.bot.allowed_channels.contains(&command.channel_id.get()) {
            let message = CreateInteractionResponseMessage::new()
                .content("このチャンネルは集計の対象外です。")
                .ephemeral(true);
            let _ = command
                .create_response(&ctx.http, CreateInteractionResponse::Message(message))
                .await;
            return;
        }

        let choice = command
            .data
            .options
            .first()
            .and_then(|o| o.value.as_str())
            .unwrap_or("week")
            .to_string();
        let span = manual_span(&choice, now_unix());

        // 履歴の取得に数秒かかることがあるので、先に「処理中」を返しておく。
        // これをしないと Discord 側が3秒で応答なしと判断してしまう。
        if let Err(e) = command.defer(&ctx.http).await {
            eprintln!("応答の保留に失敗: {:?}", e);
            return;
        }

        let text = match build_report(&ctx.http, &self.bot.enricher, command.channel_id, &span).await
        {
            Ok(text) => text,
            Err(e) => {
                eprintln!("集計に失敗: {:?}", e);
                "集計中にエラーが発生しました。".to_string()
            }
        };

        let followup = CreateInteractionResponseFollowup::new().content(truncate(&text, MAX_CONTENT));
        if let Err(e) = command.create_followup(&ctx.http, followup).await {
            eprintln!("結果の送信に失敗: {:?}", e);
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} がオンラインになりました！", ready.user.name);

        // 対象チャンネルが属するサーバーにスラッシュコマンドを登録する。
        // サーバー単位の登録は即座に反映される（全体登録は反映に最大1時間かかる）。
        let mut registered = HashSet::new();
        for channel_id in &self.bot.allowed_channels {
            match ctx.http.get_channel(ChannelId::new(*channel_id)).await {
                Ok(Channel::Guild(channel)) => {
                    if !registered.insert(channel.guild_id) {
                        continue;
                    }
                    match channel
                        .guild_id
                        .create_command(&ctx.http, ranking_command())
                        .await
                    {
                        Ok(_) => println!("/ranking を登録しました（サーバー {}）", channel.guild_id),
                        Err(e) => eprintln!("/ranking の登録に失敗: {:?}", e),
                    }
                }
                Ok(_) => eprintln!("チャンネル {} はサーバー内のチャンネルではありません", channel_id),
                Err(e) => eprintln!("チャンネル {} を取得できません: {:?}", channel_id, e),
            }
        }

        // 再接続のたびに ready が呼ばれるので、定期投稿タスクは一度だけ起動する
        if !self.scheduler_started.swap(true, Ordering::SeqCst) {
            let http = Arc::clone(&ctx.http);
            let bot = Arc::clone(&self.bot);
            tokio::spawn(async move { run_scheduler(http, bot).await });
            println!("定期投稿を開始しました（毎週月曜9時・毎月1日9時 JST）");
        }
    }
}

/// 定期投稿の時刻が来ていないか、一定間隔で確認し続ける。
async fn run_scheduler(http: Arc<Http>, bot: Arc<Bot>) {
    let mut ticker = tokio::time::interval(SCHEDULER_INTERVAL);

    loop {
        ticker.tick().await;

        let now = now_unix();
        let due = [schedule::due_weekly(now), schedule::due_monthly(now)];

        for channel_id in &bot.allowed_channels {
            for span in &due {
                if let Err(e) = post_if_due(&http, &bot, ChannelId::new(*channel_id), span).await {
                    eprintln!("定期投稿に失敗（チャンネル {}）: {:?}", channel_id, e);
                }
            }
        }
    }
}

/// まだ投稿していない期間であれば、ランキングを投稿する。
///
/// 投稿済みかどうかはチャンネルの履歴を見て判断する。プロセスの記憶に頼らないため、
/// 再起動をまたいでも二重投稿しない。逆に Bot が停止していた場合は、
/// 復帰後に遅れて投稿される。
async fn post_if_due(
    http: &Http,
    bot: &Bot,
    channel_id: ChannelId,
    span: &Span,
) -> Result<(), serenity::Error> {
    let Some(key) = &span.key else {
        return Ok(());
    };

    // 投稿されるとすれば期間の終了以降なので、そこまで遡れば十分
    if history::contains_marker(http, channel_id, key, span.end).await? {
        return Ok(());
    }

    let text = build_report(http, &bot.enricher, channel_id, span).await?;
    let body = with_marker(&text, key);

    channel_id
        .send_message(http, CreateMessage::new().content(body))
        .await?;
    println!("定期ランキングを投稿しました: {} ({})", key, channel_id);

    Ok(())
}

/// 本文の末尾に、重複判定用の識別子を小さな文字で添える。
///
/// 切り詰めで識別子が消えると二重投稿につながるため、切り詰めた後に付ける。
fn with_marker(text: &str, key: &str) -> String {
    let marker = format!("\n-# {key}");
    let room = MAX_CONTENT.saturating_sub(marker.chars().count());
    format!("{}{}", truncate(text, room), marker)
}

/// `/ranking` コマンドの定義。
fn ranking_command() -> CreateCommand {
    CreateCommand::new("ranking")
        .description("動画共有のランキングを表示します")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "period",
                "集計する期間（省略時は直近7日間）",
            )
            .add_string_choice("直近7日間", "week")
            .add_string_choice("直近30日間", "month")
            .add_string_choice("全期間", "all"),
        )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `/ranking` の選択肢から集計期間を決める。
fn manual_span(choice: &str, now: i64) -> Span {
    match choice {
        "month" => schedule::rolling(now, 30, "直近30日間", "前月比"),
        "all" => schedule::all_time(now),
        _ => schedule::rolling(now, 7, "直近7日間", "前週比"),
    }
}

/// 履歴を集計して投稿用の本文を作る。
async fn build_report(
    http: &Http,
    enricher: &Enricher,
    channel_id: ChannelId,
    span: &Span,
) -> Result<String, serenity::Error> {
    // 前期比を出すため、ひとつ前の期間まで余分に遡る
    let since = span.previous_start.unwrap_or(span.start);
    let posts = history::collect_posts(http, channel_id, since).await?;

    let (mut current, previous): (Vec<_>, Vec<_>) =
        posts.into_iter().partition(|p| span.contains(p.timestamp));

    let previous_total = span.previous_start.map(|previous_start| {
        previous
            .iter()
            .filter(|p| p.timestamp >= previous_start && p.timestamp < span.start)
            .map(|p| p.tweets.len())
            .sum()
    });

    let highlights = enrich_posts(enricher, &mut current).await;

    Ok(format_report(
        &tally::tally(&current),
        span,
        previous_total,
        &highlights,
    ))
}

/// FxTwitter API で元ツイートの情報を補い、載せられる話題を組み立てる。
///
/// API が使えなくても集計そのものは成立するため、失敗しても空の結果を返す。
async fn enrich_posts(enricher: &Enricher, posts: &mut [tally::SharedPost]) -> Highlights {
    // 新しい投稿から順に問い合わせたいので、新しい順に並べてIDを集める
    let mut ordered: Vec<&tally::SharedPost> = posts.iter().collect();
    ordered.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let ids: Vec<String> = ordered
        .iter()
        .flat_map(|p| p.tweets.iter().map(|t| t.id.clone()))
        .collect();

    if ids.is_empty() {
        return Highlights::default();
    }

    let fetched = enricher.fetch_many(&ids).await;
    if fetched.is_empty() {
        return Highlights::default();
    }

    // URL に名前が入っていない投稿も、ここで正しいアカウント名になる
    let screen_names = fetched
        .iter()
        .map(|(id, info)| (id.clone(), info.screen_name.clone()))
        .collect();
    tally::apply_screen_names(posts, &screen_names);

    let top_tweet = fetched.values().max_by_key(|info| info.likes).cloned();
    let texts: Vec<&str> = fetched.values().map(|info| info.text.as_str()).collect();

    Highlights {
        top_tweet,
        hashtags: tally::count_hashtags(&texts),
    }
}

/// 文字数の上限で切り詰める。文字の途中で切らないようにする。
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
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
        bot: Arc::new(Bot {
            allowed_channels,
            enricher: Enricher::new().expect("HTTPクライアントの作成に失敗しました"),
        }),
        scheduler_started: AtomicBool::new(false),
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

    #[test]
    fn コマンドの選択肢から期間を決められる() {
        let now = 1_800_000_000;

        assert_eq!(manual_span("week", now).label, "直近7日間");
        assert_eq!(manual_span("month", now).label, "直近30日間");
        assert_eq!(manual_span("all", now).label, "全期間");
        // 未知の値や未指定は既定の週にする
        assert_eq!(manual_span("", now).label, "直近7日間");

        // 手動実行は定期投稿の重複判定に混ざらない
        assert!(manual_span("week", now).key.is_none());
    }

    #[test]
    fn 識別子を末尾に付ける() {
        let out = with_marker("本文", "週次 2026-W36");
        assert!(out.ends_with("\n-# 週次 2026-W36"));
        assert!(out.starts_with("本文"));
    }

    #[test]
    fn 本文が長くても識別子は残る() {
        // 識別子が切り落とされると二重投稿の原因になる
        let long = "あ".repeat(MAX_CONTENT * 2);
        let out = with_marker(&long, "週次 2026-W36");

        assert!(out.ends_with("週次 2026-W36"), "識別子が残っていること");
        assert!(out.chars().count() <= MAX_CONTENT, "上限を超えないこと");
    }

    #[test]
    fn 上限以内ならそのまま返す() {
        assert_eq!(truncate("あいうえお", 10), "あいうえお");
        assert_eq!(truncate("あいうえお", 5), "あいうえお");
    }

    #[test]
    fn 上限を超えたら省略記号を付ける() {
        let out = truncate("あいうえお", 3);
        assert_eq!(out, "あい…");
        // バイト数ではなく文字数で数える
        assert_eq!(out.chars().count(), 3);
    }
}