# fxtwitter-bot

指定した Discord チャンネルに投稿された Twitter / X のURLを、[FxTwitter](https://github.com/FixTweet/FxEmbed) のURL（`fxtwitter.com`）へ自動的に置き換える小さな Bot です。

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

## 必要なもの

- Rust（edition 2021）
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

| 環境変数 | 必須 | 説明 |
| --- | --- | --- |
| `DISCORD_TOKEN` | ○ | Discord Bot のトークン |

対象チャンネルのIDは現時点では `src/main.rs` の `allowed_channel` にハードコードされています。自分の環境で動かす場合はこの値を書き換えてください（Discord で開発者モードを有効にすると、チャンネルを右クリックしてIDをコピーできます）。

## ローカルでの実行

```sh
export DISCORD_TOKEN="あなたのトークン"
cargo run --release
```

## Docker

```sh
docker build -t fxtwitter-bot .
docker run -e DISCORD_TOKEN="あなたのトークン" fxtwitter-bot
```

## Fly.io へのデプロイ

`fly.toml` は東京リージョン（`nrt`）の shared-cpu-1x / 256MB で動く構成になっています。HTTP を待ち受けないワーカーとして常時稼働します。

```sh
fly launch --no-deploy          # 初回のみ（アプリ名は適宜変更してください）
fly secrets set DISCORD_TOKEN="あなたのトークン"
fly deploy
```

### GitHub Actions による自動デプロイ

`main` ブランチへの push で `.github/workflows/fly-deploy.yml` が実行され、Fly.io へデプロイされます。リポジトリの Secrets に `FLY_API_TOKEN` を登録してください。

```sh
fly tokens create deploy -x 999999h
```

## ライセンス

[MIT](LICENSE)
