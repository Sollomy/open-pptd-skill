//! Structural OOXML schema guards for the generated package.
//!
//! Every rule below corresponds to a regression that once produced a
//! "repair needed" file (see the commit that fixed the WPS anomaly):
//!
//! * `a:xfrm` — `rot` / `flipH` / `flipV` are **attributes** of the
//!   transform, never child elements;
//! * `a:ln` — `prstDash` is a **child element** that must come *after* the
//!   fill, never an attribute;
//! * text boxes carry an explicit `a:noFill`;
//! * `a:bgFillStyleLst` ships three background fills (PowerPoint habit);
//! * `p:sld` children follow the strict order `cSld → clrMapOvr → transition`;
//! * no legacy element names (`a:cTo`) survive.
//!
//! The scans parse every XML part with `xmltree`, so malformed XML also
//! fails here.

mod common;

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use open_pptd::pptd;
use open_pptd::pptx::writer::PptxWriter;
use xmltree::{Element, XMLNode};

/// Collapse `.`/`..` segments in a package-relative path, returning `None` if
/// `..` would escape above the package root.
fn normalize_rel(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            other => out.push(other),
        }
    }
    Some(out.join("/"))
}

struct BuiltDeck {
    name: &'static str,
    part_names: Vec<String>,
    xml_parts: Vec<(String, Element)>,
}

const DECK_FIXTURES: &[(&str, &str)] = &[
    (
        "buildable",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/buildable/buildable.pptd"
        ),
    ),
    (
        "tables",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tables/tables.pptd"
        ),
    ),
    (
        "chart",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/chart/chart.pptd"
        ),
    ),
    (
        "starter",
        concat!(env!("CARGO_MANIFEST_DIR"), "/example/starter/deck.pptd"),
    ),
];

fn built_decks() -> &'static [BuiltDeck] {
    static DECKS: OnceLock<Vec<BuiltDeck>> = OnceLock::new();
    DECKS.get_or_init(|| {
        let tmp = common::TempDir::new("schema-guard");
        DECK_FIXTURES
            .iter()
            .map(|(name, entry)| {
                let project = pptd::load_project(Path::new(entry))
                    .unwrap_or_else(|e| panic!("failed to load {name}: {e}"));
                let out = tmp.join(format!("{name}.pptx"));
                PptxWriter::new(&project)
                    .build(&out)
                    .unwrap_or_else(|e| panic!("failed to build {name}: {e}"));
                let file = fs::File::open(&out).unwrap();
                let mut zip = zip::ZipArchive::new(file).unwrap();
                let mut part_names = Vec::with_capacity(zip.len());
                let mut xml_parts = Vec::new();
                for i in 0..zip.len() {
                    let mut entry = zip.by_index(i).unwrap();
                    let part_name = entry.name().to_owned();
                    part_names.push(part_name.clone());
                    if part_name.ends_with(".xml") || part_name.ends_with(".rels") {
                        let mut data = Vec::new();
                        entry.read_to_end(&mut data).unwrap();
                        let root = Element::parse(data.as_slice()).unwrap_or_else(|err| {
                            panic!("{name}/{part_name} is not well-formed XML: {err}")
                        });
                        xml_parts.push((part_name, root));
                    }
                }
                BuiltDeck {
                    name,
                    part_names,
                    xml_parts,
                }
            })
            .collect()
    })
}

fn buildable_deck() -> &'static BuiltDeck {
    built_decks()
        .iter()
        .find(|d| d.name == "buildable")
        .expect("buildable fixture present")
}

/// Direct element children of `element` (skipping comments / CDATA).
fn children(element: &Element) -> Vec<&Element> {
    element
        .children
        .iter()
        .filter_map(|node| match node {
            XMLNode::Element(child) => Some(child),
            _ => None,
        })
        .collect()
}

/// Depth-first walk over every element.
fn walk<'a>(element: &'a Element, out: &mut Vec<&'a Element>) {
    out.push(element);
    for child in children(element) {
        walk(child, out);
    }
}

#[test]
fn normalize_rel_rejects_root_underflow() {
    assert_eq!(
        normalize_rel("ppt/slides/../slideLayouts/slideLayout1.xml").as_deref(),
        Some("ppt/slideLayouts/slideLayout1.xml")
    );
    assert_eq!(normalize_rel("../outside.xml"), None);
    assert_eq!(normalize_rel("ppt/../../outside.xml"), None);
}

#[test]
fn xfrm_transform_flags_are_attributes() {
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "xfrm" {
                    continue;
                }
                seen += 1;
                for forbidden in ["rot", "flipH", "flipV"] {
                    assert!(
                        children(element).iter().all(|c| c.name != forbidden),
                        "{}/{part}: <{forbidden}> must be an attribute of <a:xfrm>, not a child",
                        deck.name
                    );
                }
            }
        }
    }
    assert!(seen > 0, "guard observed zero <a:xfrm> elements");
}

#[test]
fn line_dash_is_a_child_element_after_the_fill() {
    let mut seen_ln = 0;
    let mut seen_dash = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "ln" {
                    continue;
                }
                seen_ln += 1;
                assert!(
                    !element.attributes.contains_key("prstDash"),
                    "{}/{part}: a:prstDash must not be an attribute of <a:ln>",
                    deck.name
                );
                let kids = children(element);
                if let Some(dash) = kids.iter().position(|c| c.name == "prstDash") {
                    seen_dash += 1;
                    let fill = kids.iter().position(|c| {
                        matches!(
                            c.name.as_str(),
                            "solidFill" | "noFill" | "gradFill" | "blipFill" | "pattFill"
                        )
                    });
                    if let Some(fill) = fill {
                        assert!(
                            dash > fill,
                            "{}/{part}: <a:prstDash> must come after the fill inside <a:ln>",
                            deck.name
                        );
                    }
                }
            }
        }
    }
    assert!(seen_ln > 0, "guard observed zero <a:ln> elements");
    assert!(seen_dash > 0, "guard observed zero <a:prstDash> elements");
}

#[test]
fn text_boxes_explicitly_disable_fill() {
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "sp" {
                    continue;
                }
                let sp_children = children(element);
                let is_text_box = sp_children
                    .iter()
                    .find(|c| c.name == "nvSpPr")
                    .map(|nv| {
                        let nv_children = children(nv);
                        nv_children
                            .iter()
                            .find(|c| c.name == "cNvSpPr")
                            .and_then(|c| c.attributes.get("txBox"))
                            .is_some_and(|v| v == "1")
                    })
                    .unwrap_or(false);
                if !is_text_box {
                    continue;
                }
                seen += 1;
                let sp_pr = sp_children
                    .iter()
                    .find(|c| c.name == "spPr")
                    .unwrap_or_else(|| {
                        panic!("{}/{part}: text box p:sp without p:spPr", deck.name)
                    });
                assert!(
                    children(sp_pr).iter().any(|c| c.name == "noFill"),
                    "{}/{part}: text box must carry <a:noFill> (automatic fill would paint the box)",
                    deck.name
                );
            }
        }
    }
    assert!(seen > 0, "guard observed zero text boxes");
}

#[test]
fn background_style_list_has_three_entries() {
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            if part != "ppt/theme/theme1.xml" {
                continue;
            }
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name == "bgFillStyleLst" {
                    seen += 1;
                    assert!(
                        children(element).len() >= 3,
                        "{}/{part}: a:bgFillStyleLst should ship three background fills",
                        deck.name
                    );
                }
            }
        }
    }
    assert_eq!(
        seen,
        built_decks().len(),
        "every deck must have one bgFillStyleLst"
    );
}

#[test]
fn slide_children_follow_the_schema_order() {
    let mut seen_sld = 0;
    let mut seen_transition = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            if root.name != "sld" {
                continue;
            }
            seen_sld += 1;
            let kids = children(root);
            let csld = kids
                .iter()
                .position(|c| c.name == "cSld")
                .unwrap_or_else(|| panic!("{}/{part}: <p:sld> is missing <cSld>", deck.name));
            let clrmap = kids
                .iter()
                .position(|c| c.name == "clrMapOvr")
                .unwrap_or_else(|| {
                    panic!("{}/{part}: <p:sld> is missing <clrMapOvr>", deck.name)
                });
            assert!(
                csld < clrmap,
                "{}/{part}: <cSld> must precede <clrMapOvr>",
                deck.name
            );
            if let Some(transition) = kids.iter().position(|c| c.name == "transition") {
                seen_transition += 1;
                assert!(
                    clrmap < transition,
                    "{}/{part}: <clrMapOvr> must precede <transition>",
                    deck.name
                );
            }
        }
    }
    assert!(seen_sld > 0, "guard observed zero <p:sld> roots");
    assert!(seen_transition > 0, "guard observed zero <p:transition> elements");
}

#[test]
fn no_legacy_element_names_survive() {
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                seen += 1;
                assert_ne!(
                    element.name, "cTo",
                    "{}/{part}: legacy <a:cTo> element (should be <a:cubicBezTo>)",
                    deck.name
                );
            }
        }
    }
    assert!(seen > 0, "guard observed zero XML elements");
}

#[test]
fn theme_colors_are_six_hex_digits_without_hash() {
    // ST_HexColorRGB: exactly six hex digits across EVERY XML part (theme,
    // slideMaster, slideLayout, slides, etc.), not only theme1.xml.
    let mut seen = 0;
    let mut seen_master = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "srgbClr" {
                    continue;
                }
                seen += 1;
                if part.contains("slideMaster") {
                    seen_master += 1;
                }
                let val = element
                    .attributes
                    .get("val")
                    .unwrap_or_else(|| panic!("{}/{part}: a:srgbClr without val", deck.name));
                assert!(
                    val.len() == 6 && val.chars().all(|c| c.is_ascii_hexdigit()),
                    "{}/{part}: srgbClr val `{val}` must be exactly six hex digits, no '#'",
                    deck.name
                );
            }
        }
    }
    assert!(seen > 0, "guard observed zero <a:srgbClr> elements");
    assert!(
        seen_master > 0,
        "guard must observe <a:srgbClr> inside slideMaster"
    );
}

#[test]
fn complex_script_font_is_not_empty() {
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "cs" {
                    continue;
                }
                seen += 1;
                let typeface = element
                    .attributes
                    .get("typeface")
                    .map(String::as_str)
                    .unwrap_or("");
                assert!(
                    !typeface.is_empty(),
                    "{}/{part}: <a:cs typeface=\"...\"> must not be empty",
                    deck.name
                );
            }
        }
    }
    assert!(seen > 0, "guard observed zero <a:cs> elements");
}

#[test]
fn clr_map_overrides_use_master_mapping() {
    // Mirrors the Kimi exporter: overrideClrMapping on a layout/slide without
    // its own clrMap is not the canonical form.
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for element in all {
                if element.name != "clrMapOvr" {
                    continue;
                }
                seen += 1;
                let child = children(element);
                assert_eq!(
                    child.len(),
                    1,
                    "{}/{part}: clrMapOvr must have one child",
                    deck.name
                );
                assert_eq!(
                    child[0].name, "masterClrMapping",
                    "{}/{part}: clrMapOvr should carry a:masterClrMapping",
                    deck.name
                );
            }
        }
    }
    assert!(seen > 0, "guard observed zero <p:clrMapOvr> elements");
}

#[test]
fn every_relationship_resolves_to_an_existing_part() {
    // OPC relationship targets are resolved against the source part's
    // directory. A target that points nowhere (e.g. "../../slideLayouts/..."
    // from ppt/slides/) makes the app open the package as damaged.
    let mut seen_rels = 0;
    for deck in built_decks() {
        for (part_name, root) in &deck.xml_parts {
            if !part_name.ends_with(".rels") {
                continue;
            }
            let base_dir = if part_name == "_rels/.rels" {
                Path::new("")
            } else {
                Path::new(part_name).parent().unwrap().parent().unwrap()
            };
            for rel in root.children.iter().filter_map(|node| match node {
                XMLNode::Element(e) if e.name == "Relationship" => Some(e),
                _ => None,
            }) {
                if rel.attributes.get("TargetMode").map(String::as_str) == Some("External") {
                    continue;
                }
                seen_rels += 1;
                let target = rel
                    .attributes
                    .get("Target")
                    .map(String::as_str)
                    .unwrap_or("");
                let resolved = if let Some(stripped) = target.strip_prefix('/') {
                    normalize_rel(stripped)
                } else {
                    normalize_rel(&format!("{}/{}", base_dir.display(), target))
                }
                .unwrap_or_else(|| {
                    panic!(
                        "{}/{part_name}: relationship target `{target}` escapes package root",
                        deck.name
                    )
                });
                assert!(
                    deck.part_names
                        .iter()
                        .any(|n| n == &resolved || n == &format!("./{resolved}")),
                    "{}/{part_name}: relationship {} -> `{target}` resolves to `{resolved}`, \
                     which is not a part of the package",
                    deck.name,
                    rel.attributes.get("Id").map(String::as_str).unwrap_or("?")
                );
            }
        }
    }
    assert!(seen_rels > 0, "guard observed zero internal relationships");
}

#[test]
fn text_boxes_have_zero_insets() {
    // Kimi parity: a bodyPr without explicit insets falls back to the OOXML
    // 0.1" (91440 EMU) default, which shifts text ~7px off the authored
    // position in small boxes. Line spacing is NOT forced: a PPTD paragraph
    // without `lineHeight` emits no `<a:lnSpc>` so the renderer uses its
    // natural default (QuickLook/WPS ≈ 1.2× font), matching a source box
    // that carried no `<a:lnSpc>` (forcing `spcPct=120000` rendered tight).
    let deck = buildable_deck();
    let root = deck
        .xml_parts
        .iter()
        .find(|(name, _)| name.ends_with("slides/slide1.xml"))
        .map(|(_, r)| r)
        .expect("deck has slide1");
    let mut sps = Vec::new();
    walk(root, &mut sps);
    let sps: Vec<&Element> = sps.into_iter().filter(|e| e.name == "sp").collect();
    assert!(sps.len() >= 3, "guard sees no shapes (saw {})", sps.len());
    let mut found_body = 0;
    for sp in &sps {
        let Some(tx_body) = sp.children.iter().find_map(|n| match n {
            XMLNode::Element(e) if e.name == "txBody" => Some(e),
            _ => None,
        }) else {
            continue;
        };
        let Some(bp) = tx_body.children.iter().find_map(|n| match n {
            XMLNode::Element(e) if e.name == "bodyPr" => Some(e),
            _ => None,
        }) else {
            continue;
        };
        found_body += 1;
        for key in ["lIns", "rIns", "tIns", "bIns", "rtlCol"] {
            assert_eq!(
                bp.attributes.get(key).map(String::as_str),
                Some("0"),
                "text box must zero the {key} inset (kimi parity)"
            );
        }
        let p = tx_body
            .children
            .iter()
            .find_map(|n| match n {
                XMLNode::Element(e) if e.name == "p" => Some(e),
                _ => None,
            })
            .expect("text box has a paragraph");
        let ppr = p
            .children
            .iter()
            .find_map(|n| match n {
                XMLNode::Element(e) if e.name == "pPr" => Some(e),
                _ => None,
            })
            .expect("paragraph carries explicit pPr (kimi parity)");
        assert!(
            ppr.attributes.contains_key("algn"),
            "paragraph pPr carries explicit alignment (kimi parity)"
        );
        let end = p
            .children
            .iter()
            .find_map(|n| match n {
                XMLNode::Element(e) if e.name == "endParaRPr" => Some(e),
                _ => None,
            })
            .expect("paragraph pins the end-mark size (kimi parity)");
        let run_sz = p
            .children
            .iter()
            .find_map(|n| match n {
                XMLNode::Element(e) if e.name == "r" => Some(e),
                _ => None,
            })
            .and_then(|r| {
                r.children.iter().find_map(|n| match n {
                    XMLNode::Element(e) if e.name == "rPr" => e.attributes.get("sz").cloned(),
                    _ => None,
                })
            })
            .unwrap_or_default();
        assert_eq!(
            end.attributes.get("sz"),
            Some(&run_sz),
            "endParaRPr sz must equal the run sz (kimi parity)"
        );
        // No `<a:lnSpc>` when the PPTD carries no `lineHeight` — let the
        // renderer use its natural default (faithful to a source with none).
        let has_ln_spc = ppr
            .children
            .iter()
            .any(|n| matches!(n, XMLNode::Element(e) if e.name == "lnSpc"));
        assert!(!has_ln_spc, "no lnSpc when PPTD has no lineHeight");
    }
    assert!(
        found_body >= 2,
        "guard must observe text boxes (saw {found_body})"
    );
}

#[test]
fn text_nodes_have_no_formatting_whitespace() {
    // The writer indents for readability; a newline inside `<a:t>` is
    // significant whitespace in DrawingML and renders as an empty first
    // line, shifting every anchor=ctr text block down ~half a line.
    let mut seen = 0;
    for deck in built_decks() {
        for (part, root) in &deck.xml_parts {
            let mut all = Vec::new();
            walk(root, &mut all);
            for el in all {
                if el.name != "t" {
                    continue;
                }
                let text = el.get_text().unwrap_or_default();
                if part.starts_with("ppt/notesMasters/") && text.is_empty() {
                    continue;
                }
                seen += 1;
                assert!(!text.is_empty(), "{}/{part}: <t> must not be empty", deck.name);
                if el.attributes.get("space").map(String::as_str) != Some("preserve") {
                    assert_eq!(
                        text.trim(),
                        text.as_ref(),
                        "{}/{part}: <t> has surrounding whitespace without xml:space=preserve: {text:?}",
                        deck.name
                    );
                }
                assert!(
                    !text.contains('\n'),
                    "{}/{part}: <t> contains a line break: {text:?}",
                    deck.name
                );
            }
        }
    }
    assert!(seen >= 2, "guard observed {seen} <t> nodes");
}
