# Open PPTD

[English](README.md) | [日本語](README_JA.md) | [简体中文](README_ZH.md)

Create and edit native PowerPoint (`.pptx`) presentations offline through your AI assistant.

Every slide stays fully editable in PowerPoint—real text, tables, shapes, and speaker notes, never flat images. Everything runs locally on your computer without uploading your documents to the cloud, and you can also bring in existing `.pptx` files to update or restyle.

## Install

You do not need to run commands yourself. Copy the message below and send it to a local AI assistant that can run tools on your computer (such as Claude Code, Codex, Cursor, or WorkBuddy):

```text
Please install and set up the open-pptd skill for me:
1. Run: npx github:Sollomy/open-pptd-skill install -y
2. Run: python3 ~/.agents/skills/open-pptd/scripts/install_binary.py
3. Check that `pptd --version` works (if not found, add `$HOME/.local/bin` to `PATH`).
4. Let me know once everything is ready.
```

This one-time setup needs Node.js, Python 3, and an internet connection. If Node.js or Python 3 is not installed yet, ask your AI assistant to help install it first. Once setup finishes, making and editing slides works completely offline.

## How to Use

Describe the slides you need in plain language—or adapt the example below as a template. Your assistant will deliver a ready-to-open `.pptx` file along with a saved source folder so you can request edits later without rebuilding from scratch.

**Create a new presentation:**

```text
Use open-pptd to create an 8-slide keynote presentation on "Distributed Systems Architecture Evolution":
- Theme style: Dark Fintech (ebony-ledger) or Deep Blue Academic (deep-blue-atlas)
- Content requirements:
  1. Cover, architectural pain points, blueprint diagram (with arrowed connectors and vector cards), and performance metric comparison table;
  2. Every slide must include speaker rehearsal notes (notes: "...");
  3. Format algorithmic formulations using native LaTeX math;
- Deliverables: Complete PPTD project directory and compiled PPTX deck, verified with validate_pptx.py.
```

**Update an existing PowerPoint file:**

```text
Use open-pptd to import report.pptx, update the Q3 figures on slide 4, clean up the layout, and export report_updated.pptx.
```

## More Information

- [Full Guide & Technical Reference](docs/GUIDE.md) — slide previews, direct command-line usage, capabilities, limitations, manual installation, and test suite
- [Theme Color Palettes](theme_EN.md) — built-in styles you can ask your assistant to use by name
- [Skill Instructions](skills/open-pptd/SKILL.md) — workflow rules read automatically by your AI assistant

## License

Licensed under [GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`)](LICENSE). Upstream MIT notices (`shiquda/open-pptd-skill`, `open-kimi-ppt-skill`, `slideforge`) and Font Awesome Free 6.7.2 (`CC BY 4.0`) attributions are preserved in [LICENSE-MIT](LICENSE-MIT) and [NOTICE](NOTICE).
