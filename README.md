# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

Offline declarative PowerPoint (`.pptx`) compiler and AI coding agent skill. Author slide decks in YAML (`.pptd` + `.page`), compile them to editable native `.pptx` with the Rust `pptd` binary, or convert existing `.pptx` files back to PPTD.

## Install

Copy and send this prompt to your AI coding agent (Codex, Claude Code, Cursor, etc.):

```text
Install and configure the open-pptd skill:
1. Run: npx github:Sollomy/open-pptd-skill install -y
2. Run: python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. Verify with `pptd --version` (if not found, add `$HOME/.local/bin` to `PATH`).
4. Report back once ready.
```

Setup requires Node.js, Python 3, and network access. Once `pptd` is installed, building and converting decks runs fully offline.

## Usage

### With an AI agent

Describe your topic, slide count, and preferred style. The agent produces both the editable PPTD source folder and the compiled `.pptx` file.

```text
Use open-pptd to build an 8-slide presentation on "Distributed Systems Architecture":
- Theme: deep-blue-atlas
- Include a cover, architecture diagram with arrowed connectors, a metrics table, and speaker notes on every slide
- Verify the output with validate_pptx.py
```

### Direct CLI

```bash
pptd check my_deck/deck.pptd
pptd build my_deck/deck.pptd -o slides.pptx
pptd convert slides.pptx ./my_deck/
python3 ~/.agents/skills/open-pptd/scripts/validate_pptx.py slides.pptx --expected-pages 8 --check-notes
```

## Documentation

- [Full Guide & Technical Reference](docs/GUIDE.md) — showcase, install model, capabilities, boundaries, manual CLI setup, and test suite
- [Agent Skill Instructions](skills/open-pptd/SKILL.md)
- [PPTD v2 Syntax Specification](skills/open-pptd/reference/pptd.md)
- [Theme Palettes](theme_EN.md)
- [CLI Reference](skills/open-pptd/reference/cli.md)

## License

Licensed under the [GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`)](LICENSE). Upstream MIT notices (`shiquda/open-pptd-skill`, `open-kimi-ppt-skill`, `slideforge`) and Font Awesome Free 6.7.2 (`CC BY 4.0`) attributions are preserved in [LICENSE-MIT](LICENSE-MIT) and [NOTICE](NOTICE).
