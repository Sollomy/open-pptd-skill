# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

AI アシスタントを使って、オフラインで編集可能な PowerPoint（`.pptx`）資料を作成・修正できるツールです。

生成されるスライドは画像ではなく PowerPoint 標準のテキスト・表・図形・発表者ノートで構成されるため、そのまま自由に手直しできます。処理はすべてご自身のパソコン内で完結し、資料を外部クラウドへ送信しません。既存の `.pptx` ファイルを取り込んで内容を更新・整形することもできます。

## インストール

ご自身でコマンドを入力する必要はありません。下の文章をそのままコピーし、パソコン上の操作に対応した AI アシスタント（Claude Code、Codex、Cursor、WorkBuddy など）に送ってください。

```text
open-pptd スキルをインストールして使えるようにしてください：
1. 次を実行：npx github:Sollomy/open-pptd-skill install -y
2. 次を実行：python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. `pptd --version` で動作確認してください（見つからない場合は `$HOME/.local/bin` を `PATH` に追加してください）。
4. 準備ができたら教えてください。
```

初回セットアップにのみ Node.js、Python 3、およびインターネット接続が必要です。Node.js や Python 3 がまだ入っていない場合は、先に導入を手伝うよう AI アシスタントへ頼んでください。設定が終われば、資料の作成と修正は完全にオフラインで行えます。

## 使い方

テーマや枚数、好みの雰囲気を普段どおりの言葉で伝えるか、下の例をひな形として書き換えてお使いください。そのまま開ける `.pptx` ファイルと、後から部分修正しやすい作業用フォルダの両方が保存されます。

**新しい資料を作る例：**

```text
open-pptd を使って「分散システムアーキテクチャの進化」をテーマにした8枚のプレゼン資料を作成してください：
- テーマスタイル：ダークフィンテック調（ebony-ledger）またはディープブルー学術調（deep-blue-atlas）
- 内容要件：
  1. 表紙、アーキテクチャの課題、構成ブループリント図（矢印付きコネクタとベクターカードを含む）、性能指標の比較表を含めること
  2. すべてのスライドに発表練習用のスピーカーノート（notes: "..."）を含めること
  3. 主要な数式・アルゴリズムはネイティブ LaTeX 数式で記述すること
- 成果物：PPTD プロジェクト一式とコンパイル済みの PPTX ファイルを作成し、完了後に validate_pptx.py で品質チェックを通すこと
```

**既存の PowerPoint ファイルを修正する例：**

```text
open-pptd を使って report.pptx を読み込み、4枚目の第3四半期の数字を更新し、レイアウトを整えて report_updated.pptx として書き出してください。
```

## 詳しい情報

- [詳細ガイド・技術リファレンス（英語）](docs/GUIDE.md) — 作例プレビュー、コマンドラインでの直接実行、機能一覧、制約事項、手動インストール、テスト手順
- [テーマ配色一覧（英語）](theme_EN.md) — 名前で指定できる組み込みデザイン配色
- [スキル手順書](skills/open-pptd/SKILL.md) — AI アシスタントが自動で参照する作成ルール

## ライセンス

本プロジェクトは [GNU Affero General Public License v3.0 以降（`AGPL-3.0-or-later`）](LICENSE) の下で提供されています。上流 MIT ライセンス表記（`shiquda/open-pptd-skill`、`open-kimi-ppt-skill`、`slideforge`）および Font Awesome Free 6.7.2（`CC BY 4.0`）の帰属表示は [LICENSE-MIT](LICENSE-MIT) と [NOTICE](NOTICE) に保持されています。
