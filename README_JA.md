# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

オフラインで動作する宣言的 PowerPoint（`.pptx`）コンパイラおよび AI コーディングエージェント向けスキルです。YAML（`.pptd` + `.page`）でスライドを記述し、Rust 製ネイティブバイナリ `pptd` で編集可能な `.pptx` を生成したり、既存の `.pptx` を PPTD に逆変換したりできます。

## インストール

次の指示をそのままコピーし、お使いの AI コーディングエージェント（Codex、Claude Code、Cursor など）へ送ってください。

```text
open-pptd スキルをインストールして設定してください：
1. 次を実行：npx github:Sollomy/open-pptd-skill install -y
2. 次を実行：python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. `pptd --version` で動作確認してください（見つからない場合は `$HOME/.local/bin` を `PATH` に追加してください）。
4. 完了したら報告してください。
```

初回セットアップには Node.js、Python 3、およびネットワーク接続が必要です。`pptd` の導入後は、ビルドと変換のいずれも完全にオフラインで動作します。

## 使い方

### AI エージェントで作成する

テーマ、スライド枚数、希望するスタイルをそのまま伝えてください。編集可能な PPTD ソース一式と、コンパイル済みの `.pptx` ファイルの両方が出力されます。

```text
open-pptd を使って「分散システムアーキテクチャ」に関する8枚のプレゼン資料を作成してください：
- テーマ：deep-blue-atlas
- 表紙、矢印コネクタ付きの構成図、指標比較表、全ページの発表者ノートを含めること
- 完了後、validate_pptx.py で品質チェックを実行すること
```

### CLI から直接実行する

```bash
pptd check my_deck/deck.pptd
pptd build my_deck/deck.pptd -o slides.pptx
pptd convert slides.pptx ./my_deck/
python3 ~/.agents/skills/open-pptd/scripts/validate_pptx.py slides.pptx --expected-pages 8 --check-notes
```

## ドキュメント

- [詳細ガイド・技術リファレンス（英語）](docs/GUIDE.md) — 作例、導入アーキテクチャ、機能一覧、制約事項、手動インストール、検証手順
- [エージェント向けスキル手順書](skills/open-pptd/SKILL.md)
- [PPTD v2 構文仕様](skills/open-pptd/reference/pptd.md)
- [テーマ配色一覧](theme_EN.md)
- [CLI リファレンス](skills/open-pptd/reference/cli.md)

## ライセンス

本プロジェクトは [GNU Affero General Public License v3.0 以降（`AGPL-3.0-or-later`）](LICENSE) の下で提供されています。上流 MIT ライセンス表記（`shiquda/open-pptd-skill`、`open-kimi-ppt-skill`、`slideforge`）および Font Awesome Free 6.7.2（`CC BY 4.0`）の帰属表示は [LICENSE-MIT](LICENSE-MIT) と [NOTICE](NOTICE) に保持されています。
