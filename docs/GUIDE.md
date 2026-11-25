# Open PPTD — Guide & Technical Reference

[Back to README](../README.md) · [日本語](../README_JA.md) · [简体中文](../README_ZH.md)

This document holds the showcase previews, architecture details, capability matrix, boundaries, manual CLI installation, and developer verification guide moved out of the top-level README.

---

## Showcase

Open PPTD delivers both an editable PPTD source directory and a native `.pptx` file.

| Xiaomi YU7 Keynote Deck | DJI Pocket 4 Product Deck |
| :---: | :---: |
| [![Xiaomi YU7 Presentation](images/example-yu7-pptx.png)](../example/yu7-ppt/) | [![DJI Pocket 4 Presentation](images/example-dji-pocket4-pptx.png)](../example/dji-pocket4/) |

| DeepSeek Liquid Glass Deck | Starter Template (Tables / Lines / Notes) |
| :---: | :---: |
| [![DeepSeek Liquid Glass](images/example-deepseek-liquid-glass.png)](images/example-deepseek-liquid-glass.png) | [![Starter Template](../example/starter/starter_expected.png)](../example/starter/) |

Example compilation:

```bash
pptd build example/yu7-ppt/yu7.pptd -o yu7.pptx
```

---

## Install Model & Security

Installation separates two responsibilities:

1. **Skill distribution (`npx`)**: `npx github:Sollomy/open-pptd-skill install -y` copies `SKILL.md`, `reference/`, and helper scripts into your agent skills directory (`~/.agents/skills/open-pptd` by default). Alternatively, `npx skills add https://github.com/Sollomy/open-pptd-skill -g -y` installs via the third-party `skills` CLI (full repository clone). Neither command builds slides or runs during compilation.
2. **Native compiler (`pptd`)**: `python3 ~/.agents/skills/open-pptd/scripts/install_binary.py` installs the standalone Rust binary. It pins a release version (`VERSION`, currently `1.4.2`), verifies the downloaded archive byte-for-byte against `SHA256SUMS.txt`, and falls back to `cargo install --tag v<version>` if downloads are unavailable.

Remote-pipe installation (`curl ... | bash`) is neither provided nor recommended.

---

## Why Open PPTD

| Dimension | Open PPTD | Programmatic OOXML (e.g. pptxgenjs) | Whole-Page Image PPT | Web HTML Slides |
| --- | --- | --- | --- | --- |
| Default deliverables | PPTD source project + native `.pptx` | Usually `.pptx` only | Single bitmap embedded in `.pptx` | Single HTML file |
| Agent authoring | Declarative YAML with strict AST validation | Verbose coordinates and brittle APIs | Prompt-dependent image generation | Heavy HTML/CSS layout constraints |
| PowerPoint editing | 100% vector, native text and shapes | Editable, fragile layout adjustments | Bitmap only; text not editable | Not a native `.pptx` |
| Build speed & runtime | <0.05s native Rust, offline | 1–5s (Node.js runtime) | 10–30s (image model queue) | Requires browser runtime |
| Math typography | Native OOXML math (`<a14:m>`) + Unicode fallback; hard error on unsupported commands | Unsupported or plugin-based | Rasterized text | MathJax / KaTeX |
| Privacy & network | Zero network calls for `check` / `build` / `convert` | Local runtime | Uploads slide text to cloud model | Often loads external web assets |

---

## Capabilities

- **DrawingML vector shapes**: `rect`, `roundRect`, `ellipse`, `triangle`, `diamond`, and the full OOXML preset shape set (`reference/shapes.md`), plus custom SVG paths, solid/gradient fills, borders, and drop shadows.
- **Connectors and arrows**: Relative vector coordinates (`points: "0,0 dx,dy"`), smooth cubic Bezier curves, and start/end arrowheads (`triangle`, `stealth`, etc.).
- **Native OOXML math (`<a14:m>`)**: `<latex>...</latex>` and inline `\(...\)` compile to editable Office Math equations supporting fractions (`\frac`), radicals (`\sqrt`), scripts (`^`/`_`), big operators (`\sum`, `\int`, `\lim`), matrices, multi-line alignment (`align`/`aligned`/`gather`/`split` via `<m:eqArr>`), and per-formula macros (`\newcommand`/`\def`). Standalone formulas emit display math; inline formulas carry an `mc:Fallback` Unicode rendering for readers without `a14` support (such as LibreOffice). Unsupported commands fail validation and build loudly.
- **Bi-directional native tables**: `rowSpan`/`colSpan` merges, column width ratios, row heights, per-cell styling, borders, margins, autofit, and round-trip conversion.
- **Speaker notes**: Page-level `notes` fields generate standard OOXML `notesSlide` parts and relationships.
- **Built-in vector icons**: 1,422 Font Awesome 6.7.2 Solid vector icons embedded in the binary with `fa-` prefix normalization and validation.
- **`.sync.hash` overwrite guard**: Tracks SHA-256 hashes per output file to prevent silent overwrites when a `.pptx` was edited externally; override explicitly with `--force`.
- **Cross-platform font policy**: Typefaces pass through verbatim by default for lossless round-trips; set `safeFonts: true` in the manifest to map web/CJK fonts onto universally installed system fonts.
- **Optional browser editor host (`open-pptd serve`)**: Serves the local host shell (`editor/`) on `127.0.0.1:55173`. Note that unlike the offline `pptd` CLI, `serve` embeds Kimi's online `neo-ppt` canvas (`kimi.com` / `statics.moonshot.cn`) over the network and sends opened deck contents to that iframe for visual editing.

---

## Current Boundaries & Unsupported Features

To keep builds deterministic, unsupported constructs are intercepted during `pptd check` or `pptd build`:

- **Charts**: Native charts produced by `pptd convert` (`chartPart` plus optional `workbookPart`) pass through byte-for-byte. Hand-authored declarative `data`/`series` chart syntax is not implemented and is rejected. For new charts, use native tables (`elementType: table`) or render chart images via script (`elementType: image`).
- **Animations (`animations`)**: Parsed and validated in the AST, but `p:timing` XML is not emitted yet; slides build as static decks.
- **Rich-text HTML tags**:
  - Unsupported: `<ul>`, `<ol>`, `<li>` (use normal paragraphs with `1.` or `•` prefixes) and structural tags (`<h1>`–`<h6>`, `<table>`, `<div>`, `<img>`).
  - Supported: `<p>`, `<br/>`, `<strong>`/`<b>`, `<em>`/`<i>`, `<u>`, `<s>`/`<del>`, `<span style="...">`, `<a href="https://...">`, `<sup>`, `<sub>`, and `<latex>` / `\(...\)`.
- **Shape image fills**: Shapes support solid and gradient fills only (`a:blipFill` is supported on slide backgrounds). Use separate `image` elements for photos.
- **Out-of-canvas elements**: Elements completely outside slide bounds fail `pptd check` with an error; partially overflowing elements emit a warning.

---

## Project Structure & Reference Index

A canonical self-contained PPTD project directory:

```text
my_presentation/
├── deck.pptd             # Manifest: slide size, theme tokens, typography, page list
├── pages/
│   ├── 01_cover.page     # Single-slide AST (background, text, shapes, tables, lines)
│   ├── 02_problem.page
│   └── 03_solution.page
└── assets/               # Local images and media assets (convert writes to media/)
```

Reference documents:

- [Agent Quickstart Guide (SKILL.md)](../skills/open-pptd/SKILL.md)
- [PPTD v2 Syntax Specification](../skills/open-pptd/reference/pptd.md)
- [CLI Reference](../skills/open-pptd/reference/cli.md)
- [Font & Safe-Fallback Reference](../skills/open-pptd/reference/fonts.md)
- [Tables Reference](../skills/open-pptd/reference/tables.md)
- [Shapes Reference](../skills/open-pptd/reference/shapes.md)
- [Theme Palettes (English)](../theme_EN.md) · [Theme Palettes (Chinese)](../theme.md)

---

## Manual CLI Installation

To install the `pptd` compiler standalone without an AI agent:

```bash
# Option A (recommended): bundled Python installer (pinned release + mandatory SHA-256)
python3 skills/open-pptd/scripts/install_binary.py

# Option B: build from source with Cargo (this repository)
cargo install --path .

# Option C: build pinned release tag from upstream
cargo install --git https://github.com/shiquda/open-pptd-skill.git --tag v1.4.2
```

For manual archive downloads from a GitHub Release, download `pptd-<version>-<os>.tar.gz` (`.zip` on Windows) and `SHA256SUMS.txt`, verify with `sha256sum -c SHA256SUMS.txt --ignore-missing` (or `shasum -a 256 <archive>`), extract `pptd` into `~/.local/bin`, and ensure it is executable.

---

## Developer Verification & Regression Suite

Run targeted suites according to the layer changed, or run the full CI suite for cross-layer edits:

| Layer Changed | Command | Coverage |
| --- | --- | --- |
| Node CLI, skill installer, editor server/lib (`bin/`, `lib/`, `editor/`, `test/`) | `npm test` | 28 Node tests (uses `PPTD_BIN` stubs; no Rust binary required) |
| Python QA & installer scripts (`skills/open-pptd/scripts/`, `skills/open-pptd/tests/`) | `python3 -W error -m unittest discover -s skills/open-pptd/tests -v` | 28 Python tests with zero-warning enforcement |
| Rust compiler, validator, reverse compiler (`src/`, `tests/*.rs`) | `cargo clippy --all-targets -- -D warnings && cargo test --all-targets` | Clippy + 199 Rust unit & integration tests (including OOXML schema guards) |
| End-to-end real decks (`tests/decks/`) | `PPTD=target/debug/pptd ./tests/decks/verify_all.sh` | 12 decks / 107 slides (`check` → `build` → `validate_pptx.py`) |
| Visual slide rasterization | `RENDER=1 PPTD=target/debug/pptd ./tests/decks/verify_all.sh` | Adds LibreOffice (`soffice`) PDF export, `pdftoppm` PNGs, and contact sheets |

Full CI command (mirrors `.github/workflows/ci.yml`):

```bash
cargo clippy --all-targets -- -D warnings \
  && cargo test --all-targets \
  && npm test \
  && python3 -W error -m unittest discover -s skills/open-pptd/tests -v
```

Build outputs from `verify_all.sh` land in each deck's git-ignored `build/` directory. See [`tests/decks/README.md`](../tests/decks/README.md) for deck inventory and regression notes.

---

## License & Upstream Attributions

This repository is licensed under the [GNU Affero General Public License v3.0 or later (`AGPL-3.0-or-later`)](../LICENSE). Upstream MIT copyright and permission notices are preserved in [LICENSE-MIT](../LICENSE-MIT) and [NOTICE](../NOTICE).

Upstream projects and assets:

1. **`shiquda/open-pptd-skill`** (MIT License, [GitHub](https://github.com/shiquda/open-pptd-skill)) — upstream repository from which this fork originated.
2. **`open-kimi-ppt-skill`** by Binaryify Zhuang (MIT License, [GitHub](https://github.com/acnlie/open-kimi-ppt-skill)) — foundational PPTD declarative specification and AI agent skill design.
3. **`slideforge`** by meatRoll (MIT License, [GitHub](https://github.com/meatRoll/slideforge)) — original native Rust compiler engine, extended in Open PPTD with rich text, native LaTeX math, speaker notes, embedded icons, and static validation.
4. **Font Awesome Free 6.7.2 (Solid Icons)** — embedded vector icon dataset licensed under [CC BY 4.0](https://fontawesome.com/license/free) (not replaced by `AGPL-3.0-or-later`).
