# fxtwitter-bot

指定した Discord チャンネルに投稿された Twitter / X のURLを、[FxEmbed](https://github.com/FixTweet/FxTwitter) のURL（`fxtwitter.com`）へ自動的に置き換える小さな Bot です。

Discord の標準の埋め込みでは動画がインラインで再生できないことがありますが、`fxtwitter.com` に置き換えると動画が再生・保存しやすくなります。

## 動作

対象チャンネルに Twitter / X のURLを含むメッセージが投稿されると、Bot は次の処理を行います。

1. 元のメッセージを削除する
2. URLを `https://fxtwitter.com/...` に置き換え、`**投稿者名**: 本文` の形式で投稿し直す

```
入力:  https://x.com/example/status/1234567890
出力:  **someone**: https://fxtwitter.com/example/status/1234567890
```

1つのメッセージに複数のURLが含まれる場合、すべて置き換えられます。Bot 自身の投稿は無視されます。

> **注意**: 「削除して投稿し直す」方式のため、元メッセージの返信関係・添付ファイル・リアクションは失われます。

## 同種の Bot との違い

同じ目的の Bot は多数存在します（[FixTweetBot](https://github.com/Kyrela/FixTweetBot) など）。このプロジェクトの位置づけは以下のとおりです。

- **元メッセージを削除して置き換える**方式。返信で追記する Bot と違い、チャンネルにリンクが二重に並びません
- **Rust 製・単一ファイル・依存3つ**の最小構成。動作が1つだけなので壊れる余地が小さい
- **自分でホストする前提**。他人が運用する Bot にメッセージの読み取り・削除権限を渡さずに済みます
- Fly.io の無料枠（shared-cpu-1x / 256MB）で動く軽さ

多機能さを求める場合は、既存の公開 Bot のほうが適しています。

## 必要なもの

- Rust（edition 2021。Docker / CI では 1.98 を使用）
- Discord Bot アカウント

### Discord 側の設定

[Discord Developer Portal](https://discord.com/developers/applications) で Bot を作成し、以下を設定してください。

**特権インテント**（Bot → Privileged Gateway Intents）

- `MESSAGE CONTENT INTENT` を **有効化**（メッセージ本文を読むために必須）

**Bot に必要な権限**

| 権限 | 用途 |
| --- | --- |
| メッセージを送信 (Send Messages) | 置換後のメッセージの投稿 |
| メッセージの管理 (Manage Messages) | 元メッセージの削除 |

「メッセージの管理」がないと削除に失敗し、置換後のメッセージも投稿されません。

## 設定

すべて環境変数で指定します。いずれか一方でも欠けていると、Bot は起動時にエラーで停止します。

| 環境変数 | 必須 | 説明 |
| --- | --- | --- |
| `DISCORD_TOKEN` | ○ | Discord Bot のトークン |
| `ALLOWED_CHANNEL_IDS` | ○ | 対象チャンネルのID。カンマ区切りで複数指定可 |

`ALLOWED_CHANNEL_IDS` の例:

```sh
ALLOWED_CHANNEL_IDS="123456789012345678"                       # 1チャンネル
ALLOWED_CHANNEL_IDS="123456789012345678,987654321098765432"    # 複数チャンネル
```

チャンネルIDは、Discord の設定で開発者モードを有効にすると、チャンネルを右クリックしてコピーできます。

## ローカルでの実行

```sh
export DISCORD_TOKEN="あなたのトークン"
export ALLOWED_CHANNEL_IDS="対象チャンネルのID"
cargo run --release
```

## テスト

```sh
cargo test
```

## Docker

```sh
docker build -t fxtwitter-bot .
docker run \
  -e DISCORD_TOKEN="あなたのトークン" \
  -e ALLOWED_CHANNEL_IDS="対象チャンネルのID" \
  fxtwitter-bot
```

ビルドは2段構成です。`rust:1.98-bookworm` でコンパイルし、実行ファイルだけを `debian:bookworm-slim` へコピーします。実行側と同じ `bookworm` を明示しているのは、glibc のバージョンを揃えて実行時エラーを避けるためです。バージョンを変更する際は、両者の Debian を揃えてください。

## Fly.io へのデプロイ

`fly.toml` は東京リージョン（`nrt`）の shared-cpu-1x / 256MB で動く構成です。HTTP を待ち受けないワーカーとして常時稼働します。

```sh
fly launch --no-deploy          # 初回のみ（アプリ名は適宜変更してください）
fly secrets set DISCORD_TOKEN="あなたのトークン"
fly secrets set ALLOWED_CHANNEL_IDS="対象チャンネルのID"
fly deploy
```

`ALLOWED_CHANNEL_IDS` は機密情報ではありませんが、環境ごとに異なる値なので `fly secrets` で渡し、リポジトリには含めていません。

### GitHub Actions による自動デプロイ

`main` ブランチへの push で `.github/workflows/fly-deploy.yml` が実行され、Fly.io へデプロイされます。

リポジトリの **Settings → Secrets and variables → Actions** に `FLY_API_TOKEN` を登録してください。アカウント全体のトークンではなく、アプリに限定したデプロイトークンを推奨します。

```sh
fly tokens create deploy -x 8760h    # このアプリのデプロイのみ可能・有効期限1年
```

デプロイに使う action はコミットSHAで固定しています。中身が予告なく差し替わることを防ぐためです。更新する場合は、対象リポジトリのタグに対応するSHAへ書き換えてください。

## ライセンス

[MIT](LICENSE)
