//! Integration tests: semantic validation of loaded PPTD projects.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use open_pptd::pptd;
use open_pptd::pptd::validate::validate_project;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/demo/demo.pptd");

/// Write a minimal deck into a unique RAII temp directory and return `(dir, entry_path)`.
fn tmp_deck(tag: &str, deck_yaml: &str, pages: &[(&str, &str)]) -> (common::TempDir, PathBuf) {
    common::write_deck(tag, deck_yaml, pages)
}

const MINIMAL_DECK: &str = "version: v2\nsize: [960, 540]\npages:\n  - pages/p.page\n";

#[test]
fn display_only_math_environment_must_own_its_paragraph() {
    let inline_page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: '<p>text <latex>\begin{align} a &= b \end{align}</latex> more</p>'
"#;
    let (_tmp, entry) = tmp_deck("latex-align-inline", MINIMAL_DECK, &[("p.page", inline_page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d.message.contains("display-only")),
        "inline align must be reported the way the compiler rejects it: {issues:?}"
    );

    // The same environment on a line of its own is what the compiler renders as
    // display math, and `aligned` may be used inside running text.
    let display_page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: '<p><latex>\begin{align} a &= b \\ c &= d \end{align}</latex></p>'
  - elementId: t2
    elementType: text
    bounds: [0, 100, 400, 80]
    content:
      text: '<p>where \( \begin{aligned} a &= b \\ c &= d \end{aligned} \) holds</p>'
"#;
    let (_tmp, entry) = tmp_deck("latex-align-display", MINIMAL_DECK, &[("p.page", display_page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(issues.is_empty(), "display math must stay clean: {issues:?}");
}

#[test]
fn macro_definitions_are_not_reported_as_unsupported() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: '<latex>\newcommand{\R}{\mathbb{R}}\def\norm#1{\left|#1\right|} \norm{x} \in \R^n</latex>'
"#;
    let (_tmp, entry) = tmp_deck("latex-macros", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(issues.is_empty(), "user macros must validate cleanly: {issues:?}");

    let bad_page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: '<latex>\newcommand{\R}{a}\newcommand{\R}{b} \R</latex>'
"#;
    let (_tmp, entry) = tmp_deck("latex-macros-bad", MINIMAL_DECK, &[("p.page", bad_page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("already defined") && !d.is_warning()),
        "a broken macro definition must be an error: {issues:?}"
    );
}

#[test]
fn demo_fixture_reports_unsupported_chart() {
    let project = pptd::load_project(Path::new(FIXTURE)).unwrap();
    let issues = validate_project(&project);
    assert_eq!(
        issues.len(),
        1,
        "demo fixture contains 1 chart in declarative form: {issues:?}"
    );
    assert!(
        issues[0]
            .message
            .contains("chart `trend` is not supported in declarative form"),
        "{issues:?}"
    );
}

#[test]
fn missing_chart_part_is_reported() {
    let page = r#"
elements:
  - elementId: c
    elementType: chart
    bounds: [0, 0, 400, 300]
    chartPart: media/missing.xml
"#;
    let (_tmp, entry) = tmp_deck("chart-missing", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("missing file `media/missing.xml`")),
        "expected a missing-part finding, got {issues:?}"
    );
}

#[test]
fn chart_part_outside_the_project_is_rejected() {
    let page = r#"
elements:
  - elementId: c
    elementType: chart
    bounds: [0, 0, 400, 300]
    chartPart: ../outside/chart1.xml
"#;
    let (_tmp, entry) = tmp_deck("chart-escape", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("must be a project-relative path")),
        "expected a containment finding, got {issues:?}"
    );
}

#[test]
fn duplicate_element_id_is_reported() {
    let page = r#"
elements:
  - elementId: a
    elementType: text
    bounds: [0, 0, 100, 50]
    content: {text: first}
  - elementId: a
    elementType: text
    bounds: [0, 60, 100, 50]
    content: {text: second}
"#;
    let (_tmp, entry) = tmp_deck("dup-id", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("duplicate elementId")),
        "expected a duplicate-elementId finding, got {issues:?}"
    );
}

#[test]
fn non_positive_bounds_are_reported() {
    let page = r#"
elements:
  - elementId: bad
    elementType: text
    bounds: [0, 0, 0, 50]
    content: {text: nope}
"#;
    let (_tmp, entry) = tmp_deck("zero-bounds", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("positive width and height")),
        "expected a bounds finding, got {issues:?}"
    );
}

#[test]
fn missing_theme_reference_is_reported() {
    let page = r#"
elements:
  - elementId: t
    elementType: text
    bounds: [0, 0, 100, 50]
    content:
      style: "$nope"
      text: hi
"#;
    let (_tmp, entry) = tmp_deck("missing-style", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("theme.textStyles")),
        "expected a style-reference finding, got {issues:?}"
    );
}

#[test]
fn chart_row_length_mismatch_is_reported() {
    let page = r#"
elements:
  - elementId: c
    elementType: chart
    bounds: [0, 0, 400, 300]
    data:
      cols: [a, b]
      rows:
        - [1, 2]
        - [1, 2, 3]
    series:
      - type: bar
        encode: {x: a, y: b}
"#;
    let (_tmp, entry) = tmp_deck("chart-rows", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("row 1 has 3 cells, expected 2")),
        "expected a row-length finding, got {issues:?}"
    );
}

#[test]
fn unknown_chart_series_type_is_reported() {
    let page = r#"
elements:
  - elementId: c
    elementType: chart
    bounds: [0, 0, 400, 300]
    data:
      cols: [a, b]
      rows:
        - [1, 2]
    series:
      - type: radar
        encode: {category: a, y: b}
"#;
    let (_tmp, entry) = tmp_deck("unknown-series", MINIMAL_DECK, &[("p.page", page)]);
    let result = pptd::load_project(Path::new(&entry));
    let message = result
        .expect_err("unknown series type must fail to parse")
        .to_string();
    assert!(
        message.contains("unknown variant `radar`"),
        "expected the serde unknown-variant finding, got: {message}"
    );
}

#[test]
fn table_ratio_sum_mismatch_is_reported() {
    let page = r#"
elements:
  - elementId: t
    elementType: table
    bounds: [0, 0, 400, 300]
    columnWidths: [0.5, 0.6]
    rowHeights: [1.0]
    rows:
      - - text: a
        - text: b
"#;
    let (_tmp, entry) = tmp_deck("table-ratios", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("columnWidths must sum to 1")),
        "expected a ratio-sum finding, got {issues:?}"
    );
}

#[test]
fn animation_must_reference_existing_element() {
    let page = r#"
elements:
  - elementId: t
    elementType: text
    bounds: [0, 0, 100, 50]
    content: {text: hi}
animations:
  - elementId: ghost
    effect: fade-in
"#;
    let (_tmp, entry) = tmp_deck("animation-target", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("references unknown elementId")),
        "expected an animation-target finding, got {issues:?}"
    );
}

#[test]
fn undefined_theme_color_is_reported() {
    let page = r#"
elements:
  - elementId: t
    elementType: text
    bounds: [0, 0, 100, 50]
    content:
      text: hi
      color: "$nonexistentColor"
"#;
    let (_tmp, entry) = tmp_deck("undef-color", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d
            .message
            .contains("references undefined color `$nonexistentColor`")),
        "expected undefined color diagnostic, got {issues:?}"
    );
}

#[test]
fn missing_gif_file_is_reported() {
    let page = r#"
elements:
  - elementId: anim
    elementType: image
    bounds: [0, 0, 100, 50]
    src: "assets/missing_animation.gif"
"#;
    let (_tmp, entry) = tmp_deck("missing-gif", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d
            .message
            .contains("references missing file `assets/missing_animation.gif`")),
        "expected missing GIF diagnostic, got {issues:?}"
    );
}

#[test]
fn missing_image_file_is_reported() {
    let page = r#"
elements:
  - elementId: img1
    elementType: image
    bounds: [0, 0, 100, 50]
    src: "assets/nonexistent_image.png"
"#;
    let (_tmp, entry) = tmp_deck("missing-img", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d
            .message
            .contains("references missing file `assets/nonexistent_image.png`")),
        "expected missing image file diagnostic, got {issues:?}"
    );
}

#[test]
fn invalid_shape_name_is_reported() {
    let page = r#"
elements:
  - elementId: s1
    elementType: shape
    bounds: [0, 0, 100, 50]
    shapeName: "circle"
"#;
    let (_tmp, entry) = tmp_deck("bad-shape", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d
            .message
            .contains("has unknown shapeName `circle` (did you mean `ellipse`?)")),
        "expected invalid shape diagnostic with suggestion, got {issues:?}"
    );
}

#[test]
fn unsupported_rich_text_tags_are_reported() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 100, 50]
    content:
      text: "<p><ul><li>leaked list item</li></ul></p>"
"#;
    let (_tmp, entry) = tmp_deck("unsupported-tag", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("contains unsupported tag `<ul>`")),
        "expected unsupported tag diagnostic, got {issues:?}"
    );
}

#[test]
fn unknown_misspelled_fields_are_reported() {
    let page = r##"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 100, 50]
    colro: "#ff0000"
    content:
      text: "hello"
      font_sizee: 24
"##;
    let (_tmp, entry) = tmp_deck("unknown-field", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("element `t1` has unknown field `colro`")),
        "expected unknown field colro diagnostic, got {issues:?}"
    );
    assert!(
        issues.iter().any(|d| d
            .message
            .contains("text `t1` content has unknown field `font_sizee`")),
        "expected unknown field font_sizee diagnostic, got {issues:?}"
    );
}

#[test]
fn potential_text_overflow_is_reported() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 100, 20]
    content:
      fontSize: 24
      text: |
        这是一段非常非常长的文字内容，在一个只有20pt高度的极小容器框里面排版，肯定会造成严重的上下文字视觉溢出，系统应当发出明确的文本溢出警告。
"#;
    let (_tmp, entry) = tmp_deck("overflow", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("potential text overflow in `t1`")),
        "expected text overflow warning, got {issues:?}"
    );
}

#[test]
fn unsupported_latex_command_is_reported() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: 'energy <latex>\frac{E}{\foo}</latex> and \(\bar{x}\)'
"#;
    let (_tmp, entry) = tmp_deck("latex-unsupported", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    let latex_issue = issues
        .iter()
        .find(|d| d.message.contains("unsupported LaTeX command"));
    let diag = latex_issue.unwrap_or_else(|| panic!("expected LaTeX error, got {issues:?}"));
    assert!(diag.message.contains("`\\foo`"), "got {}", diag.message);
    assert!(diag.message.contains("t1"), "got {}", diag.message);
    assert!(!diag.is_warning(), "unsupported LaTeX must be an error");
    assert!(
        !issues.iter().any(|d| d.message.contains("`\\bar`")),
        "supported commands must stay clean: {issues:?}"
    );
}

#[test]
fn documented_image_fields_are_accepted() {
    // `cropShape` and `softEdge` are implemented by the writer (a:prstGeom /
    // a:softEdge) and documented in pptd.md; the field whitelist must not
    // reject them as unknown.
    let page = r#"
elements:
  - elementId: img
    elementType: image
    bounds: [0, 0, 200, 200]
    src: assets/pic.png
    cropShape: {shapeName: roundRect, adjustments: [20000]}
    softEdge: 4
    fit: {mode: cover}
"#;
    let dir = common::TempDir::new("validate-img");
    fs::create_dir_all(dir.join("pages")).unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("deck.pptd"), MINIMAL_DECK).unwrap();
    fs::write(dir.join("pages/p.page"), page).unwrap();
    fs::write(dir.join("assets/pic.png"), b"\x89PNG\r\n\x1a\n").unwrap();

    let project = pptd::load_project(&dir.join("deck.pptd")).unwrap();
    let issues = validate_project(&project);
    assert!(
        !issues.iter().any(|d| d.message.contains("unknown field")),
        "documented image fields must validate: {issues:?}"
    );
}

#[test]
fn documented_table_fill_is_accepted() {
    let page = r##"
elements:
  - elementId: tbl
    elementType: table
    bounds: [0, 0, 400, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    fill: {type: solid, color: "#F1F1F1"}
    rows:
      - [{text: a}, {text: b}]
      - [{text: c}, {text: d}]
"##;
    let (_tmp, entry) = tmp_deck("table-fill", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        !issues.iter().any(|d| d.message.contains("unknown field")),
        "table-level fill must validate: {issues:?}"
    );
}

#[test]
fn custom_fonts_in_manifest_are_accepted() {
    let deck = "version: v2\nsize: [960, 540]\ncustomFonts:\n  - family: JetBrains Mono\n    src: \"https://fonts.googleapis.com/css2?family=JetBrains+Mono\"\npages:\n  - pages/p.page\n";
    let page = "elements:\n  - elementId: t1\n    elementType: text\n    bounds: [0, 0, 400, 60]\n    content:\n      fontFamily: JetBrains Mono\n      text: \"mono 123\"\n";
    let (_tmp, entry) = tmp_deck("custom-fonts", deck, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        !issues.iter().any(|d| d.message.contains("unknown field")),
        "customFonts is documented and parsed, so it must not be reported: {issues:?}"
    );
}

#[test]
fn four_side_border_array_is_clockwise_not_uniform() {
    // `[null, null, null, null]` must clear all four sides (Clockwise with four
    // `None`s). `Border` also accepts a four-element sequence, so a `Uniform`
    // variant placed first would swallow it and emit a default 1pt border.
    let page = r#"
elements:
  - elementId: tbl
    elementType: table
    bounds: [0, 0, 400, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    rows:
      - [{text: a, border: [null, null, null, null]}, {text: b}]
      - [{text: c}, {text: d}]
"#;
    let (_tmp, entry) = tmp_deck("border-array", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let pptd::Element::Table(table) = &project.pages[0].elements[0] else {
        panic!("expected a table element");
    };
    assert_eq!(
        table.rows[0][0].border,
        Some(pptd::BorderSpec::Clockwise([None, None, None, None])),
        "four null sides must clear the border, not fall back to Uniform"
    );
}

#[test]
fn two_side_border_array_stays_vertical_horizontal() {
    let page = r##"
elements:
  - elementId: tbl
    elementType: table
    bounds: [0, 0, 400, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    rows:
      - [{text: a, border: [{style: solid, width: 1, color: "#000000"}, null]}, {text: b}]
      - [{text: c}, {text: d}]
"##;
    let (_tmp, entry) = tmp_deck("border-pair", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let pptd::Element::Table(table) = &project.pages[0].elements[0] else {
        panic!("expected a table element");
    };
    assert_eq!(
        table.rows[0][0].border,
        Some(pptd::BorderSpec::VerticalHorizontal([
            Some(pptd::Border {
                style: Some(pptd::LineStyle::Solid),
                width: Some(1.0),
                color: Some(pptd::Color("#000000".to_string())),
                gradient: None,
            }),
            None
        ])),
        "two-element arrays keep the [top-bottom, left-right] meaning"
    );
}

#[test]
fn documented_preset_shapes_validate() {
    // reference/shapes.md is the contract; anything it documents must pass
    // `check` instead of exiting 2 with "unknown shapeName".
    let names = [
        "irregularSeal1",
        "irregularSeal2",
        "nonIsoscelesTrapezoid",
        "pieWedge",
        "gear6",
        "doubleWave",
        "leftBrace",
        "rightBracket",
        "wedgeRoundRectCallout",
        "accentBorderCallout3",
        "leftRightCircularArrow",
        "swooshArrow",
        "flowChartDecision",
        "ribbon2",
        "lineInv",
    ];
    for name in names {
        let page = format!(
            "elements:\n  - elementId: s1\n    elementType: shape\n    shapeName: {name}\n    bounds: [0, 0, 100, 100]\n    fill: {{type: solid, color: \"#FF0000\"}}\n"
        );
        let (_tmp, entry) = tmp_deck(&format!("shape-{name}"), MINIMAL_DECK, &[("p.page", &page)]);
        let project = pptd::load_project(Path::new(&entry)).unwrap();
        let issues = validate_project(&project);
        assert!(
            issues.is_empty(),
            "documented shape `{name}` must validate: {issues:?}"
        );
    }

    // A genuine typo still has to fail.
    let bad = "elements:\n  - elementId: s1\n    elementType: shape\n    shapeName: irregularSeal3\n    bounds: [0, 0, 100, 100]\n";
    let (_tmp, entry) = tmp_deck("shape-typo", MINIMAL_DECK, &[("p.page", bad)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    assert!(
        validate_project(&project)
            .iter()
            .any(|d| d.message.contains("unknown shapeName")),
        "misspelled presets must still be reported"
    );
}

#[test]
fn manifest_whitelist_reads_the_entry_file_not_a_sibling() {
    // A directory may hold several manifests; the diagnostics must inspect the
    // one the caller passed, not a conventionally named neighbour.
    let (_tmp, entry) = tmp_deck("manifest-sibling", MINIMAL_DECK, &[("p.page", "elements: []\n")]);
    let dir = Path::new(&entry).parent().unwrap().to_path_buf();
    let custom = dir.join("custom.pptd");
    fs::write(
        &custom,
        "version: v2\nsize: [960, 540]\npages: [pages/p.page]\nbogusTopField: 1\n",
    )
    .unwrap();

    let project = pptd::load_project(&custom).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("unknown field `bogusTopField`")),
        "the passed manifest must be validated: {issues:?}"
    );
}

#[test]
fn flattened_line_shapes_survive_validation() {
    // PptxGenJS (and PowerPoint's own rule tool) draws hairline rules as
    // `<p:sp>` with `prstGeom prst="line"` and a zero extent on one axis.
    // `pptd convert` reproduces them faithfully, so they must not be treated
    // like a degenerate rectangle.
    let page = r#"
elements:
  - elementId: rule-h
    elementType: shape
    bounds: [40, 200, 880, 0]
    shapeName: line
    border:
      style: solid
      width: 0.75
      color: '#3A3A40'
  - elementId: rule-v
    elementType: shape
    bounds: [400, 40, 0, 300]
    shapeName: line
    border:
      style: solid
      width: 0.75
      color: '#3A3A40'
"#;
    let (_tmp, entry) = tmp_deck("flattened-line", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        !issues
            .iter()
            .any(|d| d.message.contains("positive width and height")),
        "zero-extent line presets are legal: {issues:?}"
    );
}

#[test]
fn zero_area_rectangles_are_still_rejected() {
    let page = r#"
elements:
  - elementId: invisible
    elementType: shape
    bounds: [40, 200, 0, 0]
    shapeName: rect
    fill:
      type: solid
      color: '#FF0000'
"#;
    let (_tmp, entry) = tmp_deck("zero-rect", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("positive width and height")),
        "a zero-area rect is a modelling mistake: {issues:?}"
    );
}

#[test]
fn nested_unknown_fields_in_image_crop_shape_are_reported() {
    let page = r#"
elements:
  - elementId: img1
    elementType: image
    bounds: [0, 0, 100, 100]
    src: assets/pic.png
    cropShape:
      shapeName: roundRect
      bogusField: 123
"#;
    let dir = common::TempDir::new("validate-cropshape");
    fs::create_dir_all(dir.join("pages")).unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("deck.pptd"), MINIMAL_DECK).unwrap();
    fs::write(dir.join("pages/p.page"), page).unwrap();
    fs::write(dir.join("assets/pic.png"), b"\x89PNG\r\n\x1a\n").unwrap();

    let project = pptd::load_project(&dir.join("deck.pptd")).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d.message.contains("image `img1` cropShape has unknown field `bogusField`")),
        "expected cropShape unknown field diagnostic, got {issues:?}"
    );
}

#[test]
fn nested_unknown_fields_in_table_cells_are_reported() {
    let page = r##"
elements:
  - elementId: tbl1
    elementType: table
    bounds: [0, 0, 400, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    rows:
      - - text: "cell 1"
          bogusCellField: true
        - text: "cell 2"
      - - text: "cell 3"
        - text: "cell 4"
"##;
    let (_tmp, entry) = tmp_deck("table-cell-unknown", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d.message.contains("table `tbl1` cell has unknown field `bogusCellField`")),
        "expected table cell unknown field diagnostic, got {issues:?}"
    );
}

#[test]
fn nested_unknown_fields_in_animations_are_reported() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 100, 50]
    content:
      text: "hello"
animations:
  - elementId: t1
    effect: fade-in
    bogusAnimProp: 99
"#;
    let (_tmp, entry) = tmp_deck("anim-unknown", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d.message.contains("animation on page has unknown field `bogusAnimProp`")),
        "expected animation unknown field diagnostic, got {issues:?}"
    );
}

#[test]
fn batch_unknown_field_diagnostics_report_all_issues_in_single_run() {
    let deck = "version: v2\nsize: [960, 540]\nbogusManifest: 1\npages:\n  - pages/p.page\n";
    let page = r##"
bogusPageHeader: "test"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 100, 50]
    bogusElemField1: true
    bogusElemField2: false
    content:
      text: "hello"
      bogusContent1: 1
      bogusContent2: 2
"##;
    let (_tmp, entry) = tmp_deck("batch-unknown", deck, &[("p.page", page)]);
    let project = pptd::load_project(Path::new(&entry)).unwrap();
    let issues = validate_project(&project);

    assert!(issues.iter().any(|d| d.message.contains("unknown field `bogusManifest` in deck manifest")));
    assert!(issues.iter().any(|d| d.message.contains("unknown field `bogusPageHeader` on page")));
    assert!(issues.iter().any(|d| d.message.contains("element `t1` has unknown field `bogusElemField1`")));
    assert!(issues.iter().any(|d| d.message.contains("element `t1` has unknown field `bogusElemField2`")));
    assert!(issues.iter().any(|d| d.message.contains("text `t1` content has unknown field `bogusContent1`")));
    assert!(issues.iter().any(|d| d.message.contains("text `t1` content has unknown field `bogusContent2`")));
    // Ensure that none of the 6 diagnostics were skipped due to fail-fast
    let unknown_count = issues.iter().filter(|d| d.message.contains("unknown field")).count();
    assert_eq!(unknown_count, 6, "all 6 unknown fields must be reported in one pass: {issues:?}");
}

#[test]
fn single_source_known_fields_coverage() {
    use open_pptd::pptd::animation::Animation;
    use open_pptd::pptd::ast::{Page, Presentation};
    use open_pptd::pptd::chart::Chart;
    use open_pptd::pptd::elements::{
        Cell, CellMargin, Element, ElementCommon, Icon, Image, Line, Media, Shape, ShapeDef, Table,
        Text, TextContent,
    };

    assert!(!Presentation::KNOWN_FIELDS.is_empty());
    assert!(!Page::KNOWN_FIELDS.is_empty());
    assert!(!Animation::KNOWN_FIELDS.is_empty());
    assert!(!ElementCommon::KNOWN_FIELDS.is_empty());
    assert!(!TextContent::KNOWN_FIELDS.is_empty());
    assert!(!ShapeDef::KNOWN_FIELDS.is_empty());
    assert!(!CellMargin::KNOWN_FIELDS.is_empty());
    assert!(!Cell::KNOWN_FIELDS.is_empty());
    assert!(!Text::OWN_FIELDS.is_empty());
    assert!(!Shape::OWN_FIELDS.is_empty());
    assert!(!Line::OWN_FIELDS.is_empty());
    assert!(!Image::OWN_FIELDS.is_empty());
    assert!(!Media::OWN_FIELDS.is_empty());
    assert!(!Icon::OWN_FIELDS.is_empty());
    assert!(!Table::OWN_FIELDS.is_empty());
    assert!(!Chart::OWN_FIELDS.is_empty());

    // Verify Element::is_known_field dispatches correctly
    assert!(Element::is_known_field("text", "elementId"));
    assert!(Element::is_known_field("text", "content"));
    assert!(Element::is_known_field("shape", "shapeName"));
    assert!(Element::is_known_field("line", "points"));
    assert!(Element::is_known_field("image", "cropShape"));
    assert!(Element::is_known_field("image", "softEdge"));
    assert!(Element::is_known_field("table", "fill"));
    assert!(Element::is_known_field("chart", "chartPart"));
    assert!(!Element::is_known_field("text", "shapeName"));
    assert!(!Element::is_known_field("shape", "content"));
}

#[test]
fn outside_image_src_is_rejected_by_validation() {
    let outer = common::TempDir::new("outside-img-root");
    let outside_png = outer.join("secret.png");
    fs::write(&outside_png, b"\x89PNG\r\n\x1a\n").unwrap();

    let page = format!(
        "elements:\n  - elementId: img1\n    elementType: image\n    bounds: [0, 0, 100, 100]\n    src: \"{}\"\n",
        outside_png.display()
    );
    let (_tmp, entry) = tmp_deck("outside-img", MINIMAL_DECK, &[("p.page", &page)]);
    let project = pptd::load_project(&entry).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| !d.is_warning()),
        "absolute image path outside project must be rejected: {issues:?}"
    );
}

#[test]
fn bare_manifest_filename_still_enforces_page_containment() {
    let outer = common::TempDir::new("bare-manifest-escape");
    fs::write(outer.join("outside.page"), "elements: []\n").unwrap();
    let proj = outer.join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join("deck.pptd"),
        "version: v2\nsize: [960, 540]\npages:\n  - ../outside.page\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_pptd"))
        .args(["check", "deck.pptd"])
        .current_dir(&proj)
        .output()
        .expect("run pptd check with bare filename");
    assert!(
        !output.status.success(),
        "bare-filename `pptd check deck.pptd` must still reject ../outside.page; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plain_less_than_inequality_passes_validation() {
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [0, 0, 400, 80]
    content:
      text: "x < 5 and y > 3"
"#;
    let (_tmp, entry) = tmp_deck("plain-lt", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let errors: Vec<_> = validate_project(&project)
        .into_iter()
        .filter(|d| !d.is_warning())
        .collect();
    assert!(
        errors.is_empty(),
        "plain `<` inequality must not be flagged as an HTML tag: {errors:?}"
    );
}

#[test]
fn table_cells_validate_html_latex_colors_and_fills() {
    let page = r#"
elements:
  - elementId: tbl1
    elementType: table
    bounds: [0, 0, 500, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    rows:
      - - text: "<ul><li>leaked</li></ul>"
        - text: '<latex>\unknownCmd{x}</latex>'
      - - text: "bad color"
          color: "$missingTextColor"
        - text: "bad fill"
          fill: {type: solid, color: "$missingFillColor"}
"#;
    let (_tmp, entry) = tmp_deck("table-cell-qa", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| d.message.contains("<ul>")),
        "table cell leaked <ul> must be reported: {issues:?}"
    );
    assert!(
        issues.iter().any(|d| d.message.contains("\\unknownCmd")),
        "table cell unsupported LaTeX must be reported: {issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("$missingTextColor")),
        "table cell undefined color must be reported: {issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|d| d.message.contains("$missingFillColor")),
        "table cell undefined fill color must be reported: {issues:?}"
    );
}

#[test]
fn cyclic_page_groups_are_rejected_without_crashing() {
    let page = r#"
groups:
  root:
    xfrm: {off: [0, 0], ext: [400, 300], chOff: [0, 0], chExt: [400, 300]}
  g1:
    xfrm: {off: [10, 10], ext: [100, 100], chOff: [10, 10], chExt: [100, 100]}
    parent: g2
  g2:
    xfrm: {off: [20, 20], ext: [100, 100], chOff: [20, 20], chExt: [100, 100]}
    parent: g1
elements:
  - elementId: t1
    elementType: text
    groupId: g1
    bounds: [10, 10, 80, 40]
    content: {text: "loop"}
"#;
    let (_tmp, entry) = tmp_deck("cyclic-groups", MINIMAL_DECK, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues.iter().any(|d| !d.is_warning()),
        "cyclic Page.groups must produce a validation error: {issues:?}"
    );
}

#[test]
fn posterless_video_and_invalid_crop_shape_and_negative_line_are_rejected() {
    let dir = common::TempDir::new("validate-video-crop-line");
    fs::create_dir_all(dir.join("pages")).unwrap();
    fs::create_dir_all(dir.join("assets")).unwrap();
    fs::write(dir.join("deck.pptd"), MINIMAL_DECK).unwrap();
    fs::write(dir.join("assets/pic.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    fs::write(dir.join("assets/demo.mp4"), b"\x00\x00\x00\x18ftypmp42").unwrap();
    fs::write(
        dir.join("pages/p.page"),
        r#"
elements:
  - elementId: vid1
    elementType: video
    bounds: [0, 0, 200, 120]
    src: assets/demo.mp4
  - elementId: img1
    elementType: image
    bounds: [220, 0, 100, 100]
    src: assets/pic.png
    cropShape: {shapeName: notARealShape}
  - elementId: negline
    elementType: shape
    shapeName: line
    bounds: [40, 200, -10, 0]
"#,
    )
    .unwrap();

    let project = pptd::load_project(&dir.join("deck.pptd")).unwrap();
    let issues = validate_project(&project);
    assert!(
        issues
            .iter()
            .any(|d| !d.is_warning() && d.message.contains("poster")),
        "poster-less video must be rejected by check: {issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|d| !d.is_warning() && d.message.contains("notARealShape")),
        "invalid image.cropShape shapeName must be rejected: {issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|d| !d.is_warning() && d.message.contains("negline")),
        "negative-axis flattened line must be rejected: {issues:?}"
    );
}
