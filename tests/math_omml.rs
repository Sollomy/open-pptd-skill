//! Integration tests for native OOXML OMML math compilation.

mod common;

use std::path::PathBuf;

use open_pptd::pptd;
use open_pptd::pptx;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/buildable")
}

#[test]
fn deck_with_math_emits_omml_and_no_raw_latex() {
    let work = common::TempDir::new("math-omml-deck");
    let pptx_path = work.join("math_deck.pptx");
    let mut project = pptd::load_project(&fixture_dir().join("buildable.pptd")).unwrap();

    let math_text = [
        "<p><latex>\\frac{a+b}{c+d}</latex></p>",
        "<p><latex>\\frac{S \\times 8}{T}</latex></p>",
        "<p><latex>\\sqrt{x^2+y^2}</latex></p>",
        "<p><latex>\\sum_{i=1}^{n} i^2</latex></p>",
        "<p>Sentence with <latex>x^2+y^2</latex> inline.</p>",
        "<p><latex>\\text{a&lt;b&amp;c}</latex></p>",
    ]
    .join("");
    if let Some(pptd::Element::Text(t)) = project.pages[0].elements.first_mut() {
        t.common.bounds.y = 10.0;
        t.common.bounds.height = 450.0;
        t.content.font_size = Some(14.0);
        t.content.text = math_text;
    }

    pptx::writer::PptxWriter::new(&project)
        .build(&pptx_path)
        .expect("build with math formulas must succeed");

    let slide_xml = common::read_zip_part(&pptx_path, "ppt/slides/slide1.xml");

    // Must declare a14 and m namespaces on p:sld
    assert!(
        slide_xml.contains("xmlns:a14=\"http://schemas.microsoft.com/office/drawing/2010/main\""),
        "slide1.xml must declare a14 namespace"
    );
    assert!(
        slide_xml.contains("xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\""),
        "slide1.xml must declare m namespace"
    );

    // Must contain native OMML structures
    assert!(slide_xml.contains("<a14:m>"), "must contain <a14:m>");
    assert!(slide_xml.contains("<m:f>"), "must contain <m:f>");
    assert!(slide_xml.contains("<m:rad>"), "must contain <m:rad>");
    assert!(slide_xml.contains("<m:nary>"), "must contain <m:nary>");
    assert!(slide_xml.contains("<m:sSup>"), "must contain <m:sSup>");

    // Must contain XML-escaped text for \text{a<b&c}
    assert!(
        slide_xml.contains("a&lt;b&amp;c"),
        "must escape special characters in <m:t>"
    );
    assert!(
        !slide_xml.contains("<m:t>a<b"),
        "raw unescaped '<' must not exist in XML"
    );

    // Must not leak literal LaTeX commands as text
    assert!(
        !slide_xml.contains("\\frac"),
        "literal '\\frac' must not leak into slide"
    );
    assert!(
        !slide_xml.contains("\\times"),
        "literal '\\times' must not leak into slide"
    );
    assert!(
        !slide_xml.contains("\\sqrt"),
        "literal '\\sqrt' must not leak into slide"
    );
    assert!(
        !slide_xml.contains("\\sum"),
        "literal '\\sum' must not leak into slide"
    );

    // Must be well-formed XML: verify xmltree parses it cleanly
    xmltree::Element::parse(slide_xml.as_bytes()).expect("slide1.xml must be valid XML");
}

#[test]
fn unsupported_command_fails_build() {
    let work = common::TempDir::new("math-omml-unsupported");
    let pptx_path = work.join("bad.pptx");
    let mut project = pptd::load_project(&fixture_dir().join("buildable.pptd")).unwrap();

    if let Some(pptd::Element::Text(t)) = project.pages[0].elements.first_mut() {
        t.content.text = "<p><latex>\\foo{x}</latex></p>".to_string();
    }

    let result = pptx::writer::PptxWriter::new(&project).build(&pptx_path);
    assert!(result.is_err(), "build with unsupported command must fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("\\foo") || err_msg.contains("foo"),
        "error message must name the unsupported command: {err_msg}"
    );
}

#[test]
fn letter_spacing_and_text_direction_emitted() {
    let work = common::TempDir::new("math-omml-styling");
    let pptx_path = work.join("styling.pptx");
    let mut project = pptd::load_project(&fixture_dir().join("buildable.pptd")).unwrap();

    if let Some(pptd::Element::Text(t)) = project.pages[0].elements.first_mut() {
        t.content.letter_spacing = Some(2.0);
        t.content.text_direction = Some(pptd::TextDirection::Vertical);
    }

    pptx::writer::PptxWriter::new(&project)
        .build(&pptx_path)
        .expect("build must succeed");

    let slide_xml = common::read_zip_part(&pptx_path, "ppt/slides/slide1.xml");

    // vert="eaVert" on a:bodyPr
    assert!(slide_xml.contains("vert=\"eaVert\""), "{slide_xml}");
    // spc="150" on a:rPr (2.0 * 75)
    assert!(slide_xml.contains("spc=\"150\""), "{slide_xml}");
}

#[test]
fn inline_math_emits_alternate_content_with_fallback() {
    let work = common::TempDir::new("math-omml-alt");
    let pptx_path = work.join("alt_content.pptx");
    let mut project = pptd::load_project(&fixture_dir().join("buildable.pptd")).unwrap();

    let mixed_text = "<p>Formula <latex>x^2+y^2</latex> inline</p>";
    if let Some(pptd::Element::Text(t)) = project.pages[0].elements.first_mut() {
        t.content.text = mixed_text.to_string();
    }

    pptx::writer::PptxWriter::new(&project)
        .build(&pptx_path)
        .expect("build with inline math must succeed");

    let slide_xml = common::read_zip_part(&pptx_path, "ppt/slides/slide1.xml");

    // Inline formula must be wrapped in mc:AlternateContent
    assert!(
        slide_xml.contains("<mc:AlternateContent"),
        "inline math must use mc:AlternateContent"
    );
    assert!(
        slide_xml.contains("<mc:Choice Requires=\"a14\">"),
        "Choice block must require a14"
    );
    assert!(
        slide_xml.contains("<mc:Fallback>"),
        "Fallback block must be present"
    );
    // Choice branch contains native OMML
    assert!(
        slide_xml.contains("<a14:m><m:oMath>"),
        "Choice branch must contain native OMML"
    );
    // Fallback branch contains superscript baseline shift
    assert!(
        slide_xml.contains("baseline=\"30000\""),
        "Fallback branch must contain superscript baseline shift"
    );
}

#[test]
fn mathbb_v_norm_delimiters_and_lim_superscript_compile_and_roundtrip() {
    let work = common::TempDir::new("math-omml-fixes");
    let pptx_path = work.join("fixes.pptx");
    let mut project = pptd::load_project(&fixture_dir().join("buildable.pptd")).unwrap();

    let math_text = [
        r"<p><latex>\mathbb{V} \ne \mathbb{W}</latex></p>",
        r"<p><latex>\|x\| + \lfloor y \rfloor + \lceil z \rceil</latex></p>",
        r"<p><latex>\lim_{x \to 0}^+ f(x)</latex></p>",
        r"<p><latex>\lim_{n \to \infty} a_n</latex></p>",
    ]
    .join("");
    if let Some(pptd::Element::Text(t)) = project.pages[0].elements.first_mut() {
        t.common.bounds.height = 450.0;
        t.content.text = math_text;
    }

    let issues = pptd::validate_project(&project);
    let errors: Vec<_> = issues.iter().filter(|d| !d.is_warning()).collect();
    assert!(errors.is_empty(), "check must accept all formulas: {errors:?}");

    pptx::writer::PptxWriter::new(&project)
        .build(&pptx_path)
        .expect("build must accept every formula that check accepted");

    let slide_xml = common::read_zip_part(&pptx_path, "ppt/slides/slide1.xml");
    xmltree::Element::parse(slide_xml.as_bytes()).expect("slide1.xml must be valid XML");
    assert!(slide_xml.contains('𝕍'), "\\mathbb{{V}} must map to 𝕍: {slide_xml}");
    assert!(slide_xml.contains('𝕎'), "\\mathbb{{W}} must map to 𝕎: {slide_xml}");
    assert!(slide_xml.contains('‖'), "\\| must emit ‖: {slide_xml}");
    assert!(slide_xml.contains('⌊') && slide_xml.contains('⌋'), "floor delimiters missing");
    assert!(slide_xml.contains('⌈') && slide_xml.contains('⌉'), "ceil delimiters missing");
    assert!(
        !slide_xml.contains(r#"<m:chr m:val="lim""#),
        "\\lim^... must not emit invalid multi-char <m:nary> chr: {slide_xml}"
    );
    assert!(slide_xml.contains("<m:limLow>"), "\\lim must emit <m:limLow>: {slide_xml}");

    // Round-trip via convert must keep the limit expression instead of dropping it.
    let converted = work.join("converted");
    pptx::import::convert_pptx_to_pptd(&pptx_path, &converted).unwrap();
    let decompiled = pptd::load_project(&converted.join("deck.pptd")).unwrap();
    let decompiled_text = match &decompiled.pages[0].elements[0] {
        pptd::Element::Text(t) => &t.content.text,
        other => panic!("expected text element, got {other:?}"),
    };
    assert!(
        decompiled_text.contains(r"\lim_") || decompiled_text.contains("lim"),
        "decompiled OMML must preserve \\lim limit: {decompiled_text}"
    );
}
