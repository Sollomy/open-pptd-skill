//! End-to-end tests: `PptxWriter` produces a valid OPC/ZIP package.

mod common;

use std::fs;
use std::path::Path;

use open_pptd::pptd;
use open_pptd::pptx::writer::PptxWriter;

const BUILDABLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/buildable/buildable.pptd"
);

const BUILDABLE_LOGO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/buildable/media/parts/logo.png"
);

const REQUIRED_PARTS: [&str; 11] = [
    "[Content_Types].xml",
    "_rels/.rels",
    "docProps/core.xml",
    "ppt/presentation.xml",
    "ppt/_rels/presentation.xml.rels",
    "ppt/theme/theme1.xml",
    "ppt/slideMasters/slideMaster1.xml",
    "ppt/slideLayouts/slideLayout1.xml",
    "ppt/slides/slide1.xml",
    "ppt/slides/_rels/slide1.xml.rels",
    "ppt/media/image1.png",
];

#[test]
fn buildable_deck_produces_a_complete_package() {
    let project = pptd::load_project(Path::new(BUILDABLE)).unwrap();
    let tmp = common::TempDir::new("build-ok");
    let path = tmp.join("ok.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("buildable deck must build");

    let file = fs::File::open(&path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    for expected in REQUIRED_PARTS {
        assert!(
            names.iter().any(|n| n == expected),
            "missing part {expected}; got {names:?}"
        );
    }

    // Slide content sanity: text, shape, line, icon, image and the fade.
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(slide.contains("Hello PPTD"), "slide text missing");
    assert!(slide.contains("roundRect"), "shape missing");
    assert!(slide.contains("cxnSp"), "line missing");
    assert!(slide.contains("cubicBezTo"), "icon geometry missing");
    assert!(slide.contains("<pptd:icon"), "icon pptd extension missing");
    assert!(slide.contains("<p:pic>"), "image missing");
    assert!(
        slide.contains("r:embed=\"rId2\""),
        "image blip embed missing"
    );
    assert!(
        slide.contains("<a:srcRect"),
        "contain fit should pad via srcRect"
    );
    assert!(slide.contains("<p:transition"), "transition missing");
    assert!(slide.contains("<p:fade/>"), "fade transition missing");

    // Theme carries the resolved color slots.
    let theme = common::read_zip_part(&path, "ppt/theme/theme1.xml");
    assert!(theme.contains("2563EB"), "theme primary color missing");

    // Media relationship: rId1 = layout, rId2 = the png image.
    let slide_rels = common::read_zip_part(&path, "ppt/slides/_rels/slide1.xml.rels");
    assert!(
        slide_rels.contains("relationships/image"),
        "media rel missing"
    );
}

#[test]
fn content_types_cover_every_typed_part() {
    let project = pptd::load_project(Path::new(BUILDABLE)).unwrap();
    let tmp = common::TempDir::new("build-ct");
    let path = tmp.join("ct.pptx");
    PptxWriter::new(&project).build(&path).unwrap();

    let content_types = common::read_zip_part(&path, "[Content_Types].xml");
    assert!(
        content_types.contains("Extension=\"png\"") && content_types.contains("image/png"),
        "png Default content type missing"
    );
    for (part, mime) in [
        (
            "/ppt/presentation.xml",
            "presentationml.presentation.main+xml",
        ),
        ("/ppt/slides/slide1.xml", "presentationml.slide+xml"),
        ("/ppt/theme/theme1.xml", "officedocument.theme+xml"),
        (
            "/ppt/slideMasters/slideMaster1.xml",
            "presentationml.slideMaster+xml",
        ),
        (
            "/ppt/slideLayouts/slideLayout1.xml",
            "presentationml.slideLayout+xml",
        ),
        ("/docProps/core.xml", "core-properties+xml"),
    ] {
        assert!(
            content_types.contains(part) && content_types.contains(mime),
            "content type for {part} missing or wrong"
        );
    }
}

#[test]
fn rich_text_renders_instead_of_failing() {
    // PPTD rich text (`<p>`/`<span style>`/`<strong>`) is a supported feature
    // now: the subtitle must render as styled runs, not fail.
    let (work, entry) = common::write_deck(
        "rich-text",
        "version: v2\nsize: [960, 540]\npages:\n  - pages/p.page\n",
        &[(
            "p.page",
            r#"elements:
  - elementId: subtitle
    elementType: text
    bounds: [40, 40, 800, 60]
    content:
      text: <p><span style="color:#ffffff;">FY2024 · built with PPTD</span> and <strong>bold</strong></p>
"#,
        )],
    );

    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("out.pptx");
    PptxWriter::new(&project).build(&path).unwrap();

    // The styled span must produce a white run and a bold run, with no raw
    // or XML-escaped HTML markup leaking into the slide XML.
    let xml = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    for leaked in ["&lt;p", "&lt;span", "&lt;strong", "<p>"] {
        assert!(
            !xml.contains(leaked),
            "markup `{leaked}` leaked into the slide: {xml}"
        );
    }
    assert!(
        xml.contains("FY2024") && xml.contains("built with PPTD") && xml.contains("bold"),
        "subtitle text missing or mangled"
    );
    assert!(
        xml.matches("<a:rPr").count() >= 2 && xml.contains("val=\"FFFFFF\""),
        "styled run fill missing"
    );
}

#[test]
fn declarative_chart_data_is_reported() {
    // The PPTD chart DSL (data + series) cannot be compiled into chart parts
    // yet; it must fail loudly, naming the element and the page.
    let deck = "version: v2\nsize: [960, 540]\npages:\n  - pages/p.page\n";
    let page = r#"
elements:
  - elementId: bench
    elementType: text
    bounds: [0, 0, 100, 50]
    content: {text: ok}
  - elementId: c
    elementType: chart
    bounds: [0, 60, 400, 300]
    data:
      cols: [a, b]
      rows:
        - [1, 2]
    series:
      - type: bar
        encode: {x: a, y: b}
"#;
    let (work, entry) = common::write_deck("build-chart", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("chart.pptx");

    let message = PptxWriter::new(&project)
        .build(&path)
        .expect_err("declarative chart must fail the build")
        .to_string();
    assert!(message.contains("not supported"), "got: {message}");
    assert!(message.contains("`c`"), "should name the chart element");
    assert!(message.contains("page 1"), "should name the page");
}

/// Tables compile to a native `<a:tbl>` graphic frame: grid, per-cell rich
/// text, theme-driven header styling and `colSpan` merges all survive.
#[test]
fn table_deck_produces_native_tbl() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tables/tables.pptd"
    );
    let project = pptd::load_project(Path::new(fixture)).unwrap();
    let tmp = common::TempDir::new("build-table");
    let path = tmp.join("table.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("table deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(slide.contains("<a:tbl>"), "must contain a native table");
    assert_eq!(slide.matches("<a:gridCol").count(), 3, "three grid columns");
    assert_eq!(slide.matches("<a:tr ").count(), 3, "three rows");
    assert!(
        slide.contains("gridSpan=\"2\""),
        "colSpan=2 must map to gridSpan"
    );
    assert!(
        slide.contains("hMerge=\"1\""),
        "covered cell must be hMerge"
    );
    for text in ["名称", "合并单元格", "粗体"] {
        assert!(slide.contains(text), "cell text `{text}` must survive");
    }
    // Header row resolves `firstRowStyle` through the theme `$` reference.
    assert!(
        slide.contains("2F5D50"),
        "header fill must resolve $primary"
    );
    assert!(slide.contains("b=\"1\""), "header runs must be bold");
}

#[test]
fn line_arrows_and_straight_geometry() {
    let deck = r#"
version: v2
title: Line Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: l1
    elementType: line
    bounds: [100, 100, 200, 150]
    viewBox: [200, 150]
    points: "0,0 200,150"
    arrow: [arrow, stealth]
    border: {style: solid, width: 2, color: "#FF0000"}
"##;
    let (work, entry) = common::write_deck("line-arrow", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("line_arrow.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("line deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("<p:cxnSp>"),
        "straight 2pt line should be cxnSp"
    );
    assert!(
        slide.contains("prst=\"line\""),
        "straight line must have prst=line"
    );
    assert!(
        slide.contains("<a:headEnd type=\"triangle\""),
        "arrow head missing"
    );
    assert!(
        slide.contains("<a:tailEnd type=\"stealth\""),
        "stealth tail missing"
    );
}

#[test]
fn inline_tags_with_style_attributes_render_as_styled_runs() {
    // `<b style=…>` used to fall through the tag matcher and leak into the
    // slide as escaped literal text.
    let deck = r#"
version: v2
title: Rich Tag Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: t1
    elementType: text
    bounds: [40, 40, 800, 240]
    content:
      fontSize: 20
      text: |
        <p><strong style="color:#C00000">红</strong> 与 <b style="color:#0060C0; font-size:26px">蓝</b> <span style="font-size:14pt">小号</span></p>
        <p>嵌套：<strong>粗 <em>斜</em> 仍粗</strong> 尾巴</p>
"##;
    let (work, entry) = common::write_deck("richtag", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("richtag.pptx");
    PptxWriter::new(&project).build(&path).unwrap();
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");

    assert!(
        !slide.contains("&lt;strong") && !slide.contains("&lt;b style"),
        "tag markup leaked into the slide text: {slide}"
    );
    assert!(slide.contains(r#"<a:srgbClr val="C00000""#), "{slide}");
    assert!(slide.contains(r#"<a:srgbClr val="0060C0""#), "{slide}");
    assert!(
        slide.contains(r#"sz="2600""#),
        "26px run size missing: {slide}"
    );
    assert!(
        slide.contains(r#"sz="1400""#),
        "14pt inline font-size must be honoured: {slide}"
    );
    // Inspect the exact `<a:r>` carrying `仍粗`: it must keep bold and drop italic.
    let still_bold_run = slide
        .split("<a:r>")
        .find(|run| run.contains("仍粗"))
        .expect("run containing 仍粗");
    let still_bold_rpr = still_bold_run
        .split("<a:rPr")
        .nth(1)
        .and_then(|s| s.split("</a:rPr>").next())
        .expect("rPr for 仍粗");
    assert!(
        still_bold_rpr.contains(r#"b="1""#),
        "bold lost on run `仍粗` after nested </em>: {still_bold_rpr}"
    );
    assert!(
        !still_bold_rpr.contains(r#"i="1""#),
        "italic leaked onto run `仍粗` after </em>: {still_bold_rpr}"
    );
}

#[test]
fn table_shadow_is_emitted_on_the_table_properties() {
    let deck = r#"
version: v2
title: Table Shadow
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: t1
    elementType: table
    bounds: [40, 40, 600, 200]
    columnWidths: [0.5, 0.5]
    rowHeights: [0.5, 0.5]
    shadow: {blur: 8, color: "#000000", offset: [4, 4]}
    rows:
      - [{text: "A"}, {text: "B"}]
      - [{text: "1"}, {text: "2"}]
"##;
    let (work, entry) = common::write_deck("tblshadow", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("tblshadow.pptx");
    PptxWriter::new(&project).build(&path).unwrap();
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");

    let tblpr = slide
        .split("<a:tblPr")
        .nth(1)
        .and_then(|rest| rest.split("</a:tblPr>").next())
        .expect("a:tblPr");
    assert!(
        tblpr.contains("<a:effectLst>") && tblpr.contains("<a:outerShdw"),
        "table shadow must live inside a:tblPr: {tblpr}"
    );
}

#[test]
fn display_math_inherits_the_text_color() {
    let deck = r##"
version: v2
title: Math Color
size: [960, 540]
theme:
  colors:
    ink: "#F5F5F5"
pages: [pages/p.page]
"##;
    let page = r##"
pageType: content
background: {type: solid, color: "#101418"}
elements:
  - elementId: eq
    elementType: text
    bounds: [60, 120, 700, 200]
    content:
      fontSize: 20
      color: "$ink"
      text: '<latex>E = mc^2</latex>'
"##;
    let (work, entry) = common::write_deck("mathcolor", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("mathcolor.pptx");
    PptxWriter::new(&project).build(&path).unwrap();
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");

    let omml = slide.split("<m:oMath>").nth(1).expect("display math");
    assert!(
        omml.contains(r#"<a:srgbClr val="F5F5F5"/>"#),
        "math runs must carry the box colour: {omml}"
    );
}

#[test]
fn text_gradient_and_shadow_reach_the_run_properties() {
    let deck = r#"
version: v2
title: Text FX
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: t1
    elementType: text
    bounds: [40, 40, 800, 200]
    content:
      fontSize: 48
      bold: true
      gradient:
        gradientType: linear
        angle: 90
        stops:
          - {position: 0, color: "#1E3A8A"}
          - {position: 1, color: "#F59E0B"}
      shadow: {blur: 6, color: "#000000", offset: [3, 3]}
      text: 渐变文字
"##;
    let (work, entry) = common::write_deck("textfx", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("textfx.pptx");
    PptxWriter::new(&project).build(&path).unwrap();
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");

    let rpr = slide
        .split("<a:rPr")
        .nth(1)
        .and_then(|rest| rest.split("</a:rPr>").next())
        .expect("run properties");
    assert!(rpr.contains("<a:gradFill"), "text gradient missing: {rpr}");
    assert!(rpr.contains("<a:outerShdw"), "text shadow missing: {rpr}");
}

#[test]
fn smooth_line_emits_wellformed_cubic_bezier() {
    let deck = r#"
version: v2
title: Bezier Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: l1
    elementType: line
    bounds: [100, 100, 300, 100]
    viewBox: [3, 1]
    points: "0,0 1,1 2,0 3,1"
    curve: smooth
    border: {style: solid, width: 2, color: "#FF0000"}
"##;
    let (work, entry) = common::write_deck("bezier", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("bezier.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("smooth-curve line deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    let opens = slide.matches("<a:cubicBezTo>").count();
    let closes = slide.matches("</a:cubicBezTo>").count();
    assert!(opens > 0, "smooth curve must emit cubicBezTo segments");
    assert_eq!(opens, closes, "every cubicBezTo must be closed by name");
    assert!(
        !slide.contains("</a:cTo>"),
        "a:cubicBezTo must not be closed as </a:cTo> (malformed XML)"
    );

    // Every cubicBezTo point must lie in the same coordinate space as <a:path w/h>,
    // not multiplied by 12,700 EMU/pt.
    let root = xmltree::Element::parse(slide.as_bytes()).expect("valid slide XML");
    let mut stack = vec![&root];
    let mut checked_paths = 0;
    while let Some(el) = stack.pop() {
        if el.name == "path" {
            let w: i64 = el.attributes.get("w").unwrap().parse().unwrap();
            let h: i64 = el.attributes.get("h").unwrap().parse().unwrap();
            checked_paths += 1;
            let mut inner = vec![el];
            while let Some(node) = inner.pop() {
                if node.name == "pt" {
                    let x: i64 = node.attributes.get("x").unwrap().parse().unwrap();
                    let y: i64 = node.attributes.get("y").unwrap().parse().unwrap();
                    assert!(
                        x >= -w && x <= w * 2 && y >= -h && y <= h * 2,
                        "Bezier point ({x}, {y}) is out of scale for path w={w}, h={h}"
                    );
                }
                for child in &node.children {
                    if let xmltree::XMLNode::Element(c) = child {
                        inner.push(c);
                    }
                }
            }
        }
        for child in &el.children {
            if let xmltree::XMLNode::Element(c) = child {
                stack.push(c);
            }
        }
    }
    assert_eq!(checked_paths, 1, "must verify the smooth curve path");
}

#[test]
fn font_fallback_maps_misans_safely() {
    let deck = r#"
version: v2
safeFonts: true
title: Font Test
size: [960, 540]
theme:
  textStyles:
    body:
      fontFamily: MiSans
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      fontFamily: MiSans
      text: "测试安全字体回退"
"#;
    let (work, entry) = common::write_deck("font-misans", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("font_test.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("font test deck must build");

    let theme = common::read_zip_part(&path, "ppt/theme/theme1.xml");
    assert!(
        theme.contains("Microsoft YaHei"),
        "theme ea font must be Microsoft YaHei"
    );
    assert!(
        !theme.contains("typeface=\"MiSans\""),
        "theme must not have MiSans directly"
    );

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("typeface=\"Microsoft YaHei\""),
        "slide ea font must be Microsoft YaHei"
    );
    assert!(
        !slide.contains("typeface=\"MiSans\""),
        "slide must not have MiSans directly"
    );
}

#[test]
fn font_fallback_maps_source_han_serif_safely() {
    let deck = r#"
version: v2
safeFonts: true
title: Serif Font Test
size: [960, 540]
theme:
  textStyles:
    title:
      fontFamily: 思源宋体
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      fontFamily: 思源宋体
      text: "测试宋体安全回退"
"#;
    let (work, entry) = common::write_deck("font-serif", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("serif_font_test.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("serif font test deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("typeface=\"SimSun\""),
        "slide ea font must be SimSun"
    );
    assert!(
        slide.contains("typeface=\"Georgia\""),
        "slide latin font must be Georgia"
    );
    assert!(
        !slide.contains("typeface=\"思源宋体\""),
        "slide must not have 思源宋体 as latin"
    );
    assert!(
        !slide.contains("typeface=\"Source Han Serif SC\""),
        "slide must not have uninstalled Source Han Serif SC directly"
    );
}

#[test]
fn font_names_pass_through_unless_safe_fonts_is_enabled() {
    let deck = r#"
version: v2
title: Passthrough Font Test
size: [960, 540]
theme:
  textStyles:
    body:
      fontFamily: QuattrocentoSans
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      fontFamily: QuattrocentoSans
      text: "Typography must round-trip"
"#;
    let (work, entry) = common::write_deck("font-passthrough", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("font_passthrough.pptx");
    PptxWriter::new(&project)
        .build(&path)
        .expect("passthrough font deck must build");

    let theme = common::read_zip_part(&path, "ppt/theme/theme1.xml");
    assert!(
        theme.contains("typeface=\"QuattrocentoSans\""),
        "theme must preserve the source typeface: {theme}"
    );
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("typeface=\"QuattrocentoSans\""),
        "slide must preserve the source typeface: {slide}"
    );
    assert!(
        !slide.contains("typeface=\"Arial\""),
        "font name must not be rewritten by default: {slide}"
    );
}

#[test]
fn math_formula_default_upright_and_subscripts() {
    let deck = r#"
version: v2
title: Math Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: m1
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      text: |
        <p>公式 \(h(s_t) \ge 0\)</p>
"#;
    let (work, entry) = common::write_deck("math-sub", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("math_test.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("math deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert!(
        slide.contains("<a14:m><m:oMath>"),
        "native OMML inline math missing"
    );
    assert!(slide.contains("<m:sSub>"), "subscript structure missing");
    assert!(slide.contains("≥"), "ge symbol missing");
    assert!(
        slide.contains("mc:AlternateContent"),
        "inline math must use mc:AlternateContent"
    );
    assert!(
        slide.contains("mc:Fallback"),
        "inline math must include fallback"
    );
    assert!(
        slide.contains("baseline=\"-25000\""),
        "fallback must contain subscript baseline shift"
    );
}

#[test]
fn inline_math_inside_italic_tags_emits_two_omml_blocks() {
    let deck = r#"
version: v2
title: Math Italic Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: m2
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      text: |
        <p>强调 <i>\(x + y\)</i> 和普通 \(z\)</p>
"#;
    let (work, entry) = common::write_deck("math-it", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("math_it_test.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("math italic deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    let root = xmltree::Element::parse(slide.as_bytes()).expect("valid slide XML");
    let mut omath_texts: Vec<String> = Vec::new();
    fn collect_omath(el: &xmltree::Element, out: &mut Vec<String>) {
        if el.name == "oMath" {
            let mut text = String::new();
            fn collect_mt(n: &xmltree::Element, acc: &mut String) {
                if n.name == "t" {
                    if let Some(t) = n.get_text() {
                        acc.push_str(&t);
                    }
                }
                for c in &n.children {
                    if let xmltree::XMLNode::Element(ce) = c {
                        collect_mt(ce, acc);
                    }
                }
            }
            collect_mt(el, &mut text);
            out.push(text);
            return;
        }
        for c in &el.children {
            if let xmltree::XMLNode::Element(ce) = c {
                collect_omath(ce, out);
            }
        }
    }
    collect_omath(&root, &mut omath_texts);
    assert_eq!(
        omath_texts.len(),
        2,
        "expected two <m:oMath> blocks, got {omath_texts:?}"
    );
    assert_eq!(omath_texts[0], "x+y", "first formula must contain x+y");
    assert_eq!(omath_texts[1], "z", "second formula must contain z");
}

#[test]
fn speaker_notes_produce_notes_slide_and_rels() {
    let deck = "version: v2\ntitle: Notes Test\nsize: [960, 540]\npages:\n  - pages/p.page\n";
    let page = r#"
notes: |
  这是第一行演讲者备注。
  这是第二行演讲者备注。
elements:
  - elementId: t1
    elementType: text
    bounds: [50, 50, 400, 100]
    content:
      text: <p>Slide with notes</p>
"#;
    let (work, entry) = common::write_deck("notes", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("notes_test.pptx");

    PptxWriter::new(&project)
        .build(&path)
        .expect("notes deck must build");

    let notes_xml = common::read_zip_part(&path, "ppt/notesSlides/notesSlide1.xml");
    assert!(
        notes_xml.contains("这是第一行演讲者备注。"),
        "first line missing from notes: {notes_xml}"
    );
    assert!(
        notes_xml.contains("这是第二行演讲者备注。"),
        "second line missing from notes: {notes_xml}"
    );

    let slide_rels = common::read_zip_part(&path, "ppt/slides/_rels/slide1.xml.rels");
    assert!(
        slide_rels.contains("notesSlide1.xml"),
        "notesSlide rel missing from slide rels: {slide_rels}"
    );

    let notes_rels = common::read_zip_part(&path, "ppt/notesSlides/_rels/notesSlide1.xml.rels");
    assert!(
        notes_rels.contains("slide1.xml"),
        "slide rel missing from notes rels: {notes_rels}"
    );

    let content_types = common::read_zip_part(&path, "[Content_Types].xml");
    assert!(
        content_types.contains("notesSlide+xml"),
        "notesSlide content type missing: {content_types}"
    );
}

#[test]
fn rich_text_hyperlinks_write_external_relationships() {
    let deck = r#"
version: v2
title: Link Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [50, 50, 500, 120]
    content:
      text: |
        <p>See <a href="https://arxiv.org/abs/2605.27223">arXiv</a> and <a href="https://arxiv.org/abs/2605.27223">the paper</a>.</p>
"#;
    let (work, entry) = common::write_deck("hyperlink", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("hyperlink.pptx");
    PptxWriter::new(&project)
        .build(&path)
        .expect("hyperlink deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert_eq!(
        slide.matches("<a:hlinkClick").count(),
        2,
        "both linked runs need a click target: {slide}"
    );
    assert!(
        slide.contains("<a:hlinkClick r:id=\"rId2\"/>"),
        "link relationship id missing: {slide}"
    );
    let rels = common::read_zip_part(&path, "ppt/slides/_rels/slide1.xml.rels");
    assert_eq!(
        rels.matches("relationships/hyperlink").count(),
        1,
        "the same URL must be deduplicated: {rels}"
    );
    assert!(rels.contains("TargetMode=\"External\""), "{rels}");
    assert!(
        rels.contains("Target=\"https://arxiv.org/abs/2605.27223\""),
        "URL must be written verbatim: {rels}"
    );
}

#[test]
fn element_level_links_survive_build() {
    let deck = r#"
version: v2
title: Shape Link Test
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r##"
elements:
  - elementId: card
    elementType: shape
    shapeName: roundRect
    bounds: [50, 50, 300, 120]
    link: "https://example.com/source"
    fill: {type: solid, color: "#EEF2FF"}
"##;
    let (work, entry) = common::write_deck("elem-link", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("element_link.pptx");
    PptxWriter::new(&project)
        .build(&path)
        .expect("element link deck must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    assert_eq!(slide.matches("<a:hlinkClick").count(), 1, "{slide}");
    assert!(slide.contains("<a:hlinkClick r:id=\"rId2\"/>"), "{slide}");
    let rels = common::read_zip_part(&path, "ppt/slides/_rels/slide1.xml.rels");
    assert!(
        rels.contains("Target=\"https://example.com/source\""),
        "{rels}"
    );
    assert!(rels.contains("TargetMode=\"External\""), "{rels}");
}

#[test]
fn text_boxes_without_autofit_do_not_invent_shrink_to_fit() {
    // An omitted `autofit` must stay omitted: OOXML reads the absence as
    // "do not autofit". Writing `normAutofit` instead made PowerPoint shrink
    // text that the source deck rendered at its declared size, and it broke
    // round-tripping of decks that never declared an autofit mode.
    let deck = r#"
version: v2
title: Autofit
size: [960, 540]
pages: [pages/p.page]
"#;
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [40, 40, 400, 60]
    content:
      fontSize: 18
      text: plain
  - elementId: t2
    elementType: text
    bounds: [40, 140, 400, 60]
    content:
      fontSize: 18
      autofit: fit_text
      text: shrink
"#;
    let (work, entry) = common::write_deck("autofit", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("autofit.pptx");
    PptxWriter::new(&project).build(&path).unwrap();
    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");

    assert_eq!(
        slide.matches("<a:normAutofit").count(),
        1,
        "only the explicit fit_text box may declare shrink-to-fit"
    );
    assert_eq!(slide.matches("<a:spAutoFit").count(), 0);
    assert!(
        !slide.contains("<a:noAutofit"),
        "an unset autofit must not be materialised as noAutofit either"
    );
}

#[test]
fn unclosed_p_tag_and_plain_inequality_do_not_panic_build() {
    let deck = "version: v2\nsize: [960, 540]\npages: [pages/p.page]\n";
    let page = r#"
elements:
  - elementId: t1
    elementType: text
    bounds: [40, 40, 400, 60]
    content:
      text: "x <p"
  - elementId: t2
    elementType: text
    bounds: [40, 120, 400, 60]
    content:
      text: "x < 5 and y > 3"
"#;
    let (work, entry) = common::write_deck("unclosed-p", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("out.pptx");
    PptxWriter::new(&project)
        .build(&path)
        .expect("unclosed `<p` and plain inequalities must build without panicking");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    xmltree::Element::parse(slide.as_bytes()).expect("slide1.xml must be valid XML");
    assert!(slide.contains("x &lt;p"), "{slide}");
    assert!(slide.contains("x &lt; 5 and y &gt; 3"), "{slide}");
}

#[test]
fn layout_with_icon_and_math_emits_valid_namespaces() {
    let deck = r##"
version: v2
size: [960, 540]
theme:
  colors:
    accent: "#2563EB"
layouts:
  cover:
    elements:
      - elementId: ico
        elementType: icon
        bounds: [40, 40, 24, 24]
        iconName: fas:bolt
        fill: {type: solid, color: "$accent"}
      - elementId: eq
        elementType: text
        bounds: [80, 40, 300, 50]
        content:
          text: '<p>Eq <latex>x^2</latex></p>'
pages: [pages/p.page]
"##;
    let page = "layout: cover\nelements: []\n";
    let (work, entry) = common::write_deck("layout-ns", deck, &[("p.page", page)]);
    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("out.pptx");
    PptxWriter::new(&project).build(&path).unwrap();

    let layout_xml = common::read_zip_part(&path, "ppt/slideLayouts/slideLayout1.xml");
    xmltree::Element::parse(layout_xml.as_bytes())
        .expect("slideLayout1.xml with icon and math must parse cleanly");
    assert!(layout_xml.contains("<pptd:icon"), "{layout_xml}");
    assert!(layout_xml.contains("<a14:m>"), "{layout_xml}");
}

#[test]
fn custom_image_crop_shape_emits_cust_geom_and_media_uses_p_extlst() {
    let deck = r##"
version: v2
size: [960, 540]
theme:
  colors:
    cardBg: "#EEF2FF"
pages: [pages/p.page]
"##;
    let page = r#"
groups:
  g1:
    xfrm: {off: [20, 20], ext: [400, 200], chOff: [20, 20], chExt: [400, 200]}
    fill: {type: solid, color: "$cardBg"}
elements:
  - elementId: cropped
    elementType: image
    groupId: g1
    bounds: [40, 40, 120, 120]
    src: assets/logo.png
    cropShape:
      shapeName: custom
      viewBox: [100, 100]
      path: "M 50 0 L 100 50 L 50 100 L 0 50 Z"
  - elementId: clip
    elementType: video
    bounds: [200, 40, 200, 120]
    src: assets/demo.mp4
    poster: assets/logo.png
"#;
    let (work, entry) = common::write_deck("crop-media-group", deck, &[("p.page", page)]);
    fs::create_dir_all(work.join("assets")).unwrap();
    fs::copy(BUILDABLE_LOGO, work.join("assets/logo.png")).unwrap();
    fs::write(work.join("assets/demo.mp4"), b"\x00\x00\x00\x18ftypmp42").unwrap();

    let project = pptd::load_project(&entry).unwrap();
    let path = work.join("out.pptx");
    PptxWriter::new(&project)
        .build(&path)
        .expect("custom cropShape, video with poster, and themed group fill must build");

    let slide = common::read_zip_part(&path, "ppt/slides/slide1.xml");
    xmltree::Element::parse(slide.as_bytes()).expect("slide1.xml must parse cleanly");
    assert!(
        !slide.contains(r#"prst="custom""#),
        "custom cropShape must not emit invalid prst=\"custom\": {slide}"
    );
    assert!(
        slide.contains("<a:custGeom>"),
        "custom cropShape must emit <a:custGeom>: {slide}"
    );
    assert!(
        slide.contains("EEF2FF"),
        "group fill must resolve $cardBg theme token: {slide}"
    );
    // Inside <p:nvPr> for video media, extLst/ext must use the p: namespace.
    assert!(
        slide.contains("<p:extLst>") && slide.contains("<p:ext uri=\"{DAA4B4D4"),
        "media nvPr must wrap p14:media in <p:extLst><p:ext>: {slide}"
    );
    assert!(
        !slide.contains("<a:ext uri=\"{DAA4B4D4"),
        "media nvPr must not wrap p14:media in <a:extLst><a:ext>: {slide}"
    );
}
