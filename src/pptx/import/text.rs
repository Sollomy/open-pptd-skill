//! Text body extraction (`p:txBody` → `TextContent`), bullet/list defaults, and run styles.

use std::collections::BTreeMap;

use xmltree::Element as XmlEl;

use crate::pptd::elements::{TextAutofit, TextContent, TextDirection};
use crate::pptd::shared::{Alignment, Color, FontFamily, HorizontalAlign, VerticalAlign};

use super::opc::{attr, children, external_target, first, xml_bool};
use super::shapes::px;
use super::theme::{MasterDefaults, SlotColors, ThemeFonts, color_from_fill, resolve_typeface};
use super::{LossKind, PageCtx};

/// Bullet + paragraph-margin info parsed from `<a:lvl1pPr>` / `<a:pPr>`.
#[derive(Default, Clone)]
pub(super) struct BulletInfo {
    pub(super) char: Option<String>,
    pub(super) font: Option<String>,
    pub(super) margin: Option<f64>,
    pub(super) indent: Option<f64>,
}

/// `<p:style><a:fontRef idx="minor|major"> <colour/> </a:fontRef>` → the
/// shape's default text colour / font, merged onto the master `otherStyle`.
pub(super) fn font_ref_defaults(
    base: MasterDefaults,
    sp_el: &XmlEl,
    slots: &SlotColors,
) -> MasterDefaults {
    let Some(style) = first(sp_el, "style") else {
        return base;
    };
    let Some(fr) = first(style, "fontRef") else {
        return base;
    };
    let mut d = base;
    if let Some(c) = color_from_fill(fr, slots) {
        d.color = Some(c);
    }
    match attr(fr, "idx") {
        Some("minor") => d.latin_typeface = Some("+mn-lt".to_string()),
        Some("major") => d.latin_typeface = Some("+mj-lt".to_string()),
        _ => {}
    }
    d
}

/// Merge a `<a:defRPr>` onto `base` (overrides only present attributes),
/// so an `lstStyle` or paragraph `defRPr` that specifies just `sz`/`i` keeps
/// the base colour/font.
pub(super) fn merge_def_rpr(
    mut base: MasterDefaults,
    rpr: &XmlEl,
    slots: &SlotColors,
) -> MasterDefaults {
    if let Some(sz) = attr(rpr, "sz").and_then(|s| s.parse::<f64>().ok()) {
        base.sz = Some(sz / 100.0);
    }
    if let Some(c) = first(rpr, "solidFill").and_then(|sf| color_from_fill(sf, slots)) {
        base.color = Some(c);
    }
    if let Some(tf) = first(rpr, "latin")
        .and_then(|l| attr(l, "typeface"))
        .filter(|s| !s.is_empty())
    {
        base.latin_typeface = Some(tf.to_string());
    }
    if let Some(b) = xml_bool(rpr, "b") {
        base.bold = Some(b);
    }
    if let Some(i) = xml_bool(rpr, "i") {
        base.italic = Some(i);
    }
    base
}

/// `txBody > lstStyle > lvl1pPr` → merged run-style defaults (`defRPr`) plus
/// the bullet glyph / hanging indent. WPS puts the bullet on each `<a:pPr>`
/// (not the lstStyle), so paragraph-level `buChar`/`buFont`/`marL`/`indent`
/// override the lstStyle defaults; `buNone` clears the bullet. The PPTD model
/// carries one bullet per `TextContent`, so the first paragraph with a bullet
/// spec wins (uniform bulleted lists — the common case).
pub(super) fn lst_style_info(
    base: MasterDefaults,
    tx_body: &XmlEl,
    slots: &SlotColors,
) -> (MasterDefaults, BulletInfo) {
    let mut d = base;
    let mut bullet = BulletInfo::default();
    // 1. lstStyle lvl1pPr: run-style defaults + a fallback bullet.
    if let Some(lst) = first(tx_body, "lstStyle") {
        if let Some(lvl1) = first(lst, "lvl1pPr") {
            if let Some(v) = attr(lvl1, "marL").and_then(|s| s.parse::<f64>().ok()) {
                bullet.margin = Some(px(v));
            }
            if let Some(v) = attr(lvl1, "indent").and_then(|s| s.parse::<f64>().ok()) {
                bullet.indent = Some(px(v));
            }
            if let Some(bf) = first(lvl1, "buFont").and_then(|e| attr(e, "typeface")) {
                bullet.font = Some(bf.to_string());
            }
            if let Some(bc) = first(lvl1, "buChar").and_then(|e| attr(e, "char")) {
                bullet.char = Some(bc.to_string());
            }
            if let Some(rpr) = first(lvl1, "defRPr") {
                d = merge_def_rpr(d, rpr, slots);
            }
        }
    }
    // 2. Paragraph-level `<a:pPr>` overrides (the common WPS case).
    for p in children(tx_body, "p") {
        let Some(pp) = first(p, "pPr") else {
            continue;
        };
        if first(pp, "buNone").is_some() {
            bullet = BulletInfo::default();
            continue;
        }
        let mut found = false;
        if let Some(v) = attr(pp, "marL").and_then(|s| s.parse::<f64>().ok()) {
            bullet.margin = Some(px(v));
            found = true;
        }
        if let Some(v) = attr(pp, "indent").and_then(|s| s.parse::<f64>().ok()) {
            bullet.indent = Some(px(v));
            found = true;
        }
        if let Some(bf) = first(pp, "buFont").and_then(|e| attr(e, "typeface")) {
            bullet.font = Some(bf.to_string());
            found = true;
        }
        if let Some(bc) = first(pp, "buChar").and_then(|e| attr(e, "char")) {
            bullet.char = Some(bc.to_string());
            found = true;
        }
        if found {
            break;
        }
    }
    (d, bullet)
}

/// `a:bodyPr` autofit child → [`TextAutofit`]. Absent child → `None`, which
/// OOXML reads as "do not autofit" and which the writer reproduces by
/// emitting no child at all.
pub(super) fn body_pr_autofit(body_pr: Option<&XmlEl>) -> Option<TextAutofit> {
    let body_pr = body_pr?;
    if first(body_pr, "spAutoFit").is_some() {
        Some(TextAutofit::FitShape)
    } else if first(body_pr, "normAutofit").is_some() {
        Some(TextAutofit::FitText)
    } else if first(body_pr, "noAutofit").is_some() {
        Some(TextAutofit::Fixed)
    } else {
        None
    }
}

fn extract_omath(node: &XmlEl) -> Option<&XmlEl> {
    if node.name == "oMath" {
        Some(node)
    } else if node.name == "oMathPara" {
        first(node, "oMath")
    } else {
        first(node, "oMath")
            .or_else(|| first(node, "oMathPara").and_then(|omp| first(omp, "oMath")))
    }
}

/// Build `TextContent` from `p:txBody`. `None` when the box has no text.
///
/// `hoist_space_after` moves a uniform per-paragraph `a:spcAft` up to
/// [`TextContent::space_after`]; table cells pass `false` because `Cell`
/// has no equivalent field and would drop the value.
pub(super) fn text_content(
    tx_body: &XmlEl,
    rels: &BTreeMap<String, String>,
    slots: &SlotColors,
    ctx: &mut PageCtx<'_>,
    name: &str,
    hoist_space_after: bool,
) -> Option<TextContent> {
    let body_pr = first(tx_body, "bodyPr");
    let anchor = body_pr.and_then(|b| attr(b, "anchor")).map(|a| match a {
        "t" => VerticalAlign::Top,
        "ctr" => VerticalAlign::Middle,
        "b" => VerticalAlign::Bottom,
        _ => VerticalAlign::Top,
    });
    let wrap = body_pr.and_then(|b| attr(b, "wrap")).map(|w| w != "none");
    let text_direction = body_pr.and_then(|b| attr(b, "vert")).and_then(|v| match v {
        "eaVert" | "vert" | "vert270" => Some(TextDirection::Vertical),
        _ => None,
    });
    let autofit = body_pr_autofit(body_pr);
    let body_pr_extras = body_pr
        .map(|b| {
            b.attributes
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "lIns" | "tIns" | "rIns" | "bIns" | "wrap" | "rtlCol" | "anchor" | "vert"
                    )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|m| !m.is_empty());
    // Text insets: OOXML defaults when the attribute is absent are
    // lIns/rIns = 91440 EMU (7.2px) and tIns/bIns = 45720 EMU (3.6px).
    let insets = |key: &str| -> Option<f64> {
        body_pr
            .and_then(|b| attr(b, key))
            .and_then(|s| s.parse::<i64>().ok())
            .map(|v| px(v as f64))
    };
    let (margin_top, margin_left, margin_right, margin_bottom) = (
        insets("tIns").or(Some(3.6)),
        insets("lIns").or(Some(7.2)),
        insets("rIns").or(Some(7.2)),
        insets("bIns").or(Some(3.6)),
    );

    let paragraphs: Vec<&XmlEl> = children(tx_body, "p");
    if paragraphs.is_empty() {
        return None;
    }

    struct RichRun {
        style: Option<RunStyle>,
        text: String,
        link: Option<String>,
        is_math: bool,
    }

    struct RichLine {
        align: Option<HorizontalAlign>,
        line_height: Option<f64>,
        margin_top_px: Option<f64>,
        /// `a:spcAft` in points.
        space_after_pt: Option<f64>,
        runs: Vec<RichRun>,
    }

    fn css_align(a: HorizontalAlign) -> &'static str {
        match a {
            HorizontalAlign::Left => "left",
            HorizontalAlign::Center => "center",
            HorizontalAlign::Right => "right",
            HorizontalAlign::Justify => "justify",
            HorizontalAlign::Distributed => "distributed",
        }
    }

    fn escape_text(t: &str) -> String {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn escape_attr(t: &str) -> String {
        escape_text(t).replace('"', "&quot;")
    }

    let mut out_lines: Vec<RichLine> = Vec::new();
    for p in &paragraphs {
        let p_pr = first(p, "pPr");
        // A paragraph's own `defRPr` further narrows the master defaults
        // for its runs (WPS boxes regularly carry `sz=1400` there).
        let para_defaults = p_pr
            .and_then(|pp| first(pp, "defRPr"))
            .map(|dr| merge_def_rpr(ctx.defaults.clone(), dr, slots))
            .unwrap_or_else(|| ctx.defaults.clone());
        let algn = p_pr.and_then(|pp| attr(pp, "algn")).map(|a| match a {
            "ctr" => HorizontalAlign::Center,
            "r" => HorizontalAlign::Right,
            "just" => HorizontalAlign::Justify,
            "dist" => HorizontalAlign::Distributed,
            _ => HorizontalAlign::Left,
        });
        let ln = p_pr
            .and_then(|pp| first(pp, "lnSpc"))
            .and_then(|ls| first(ls, "spcPct"))
            .and_then(|sp| attr(sp, "val"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 100000.0)
            .or(ctx.para_line_spacing);
        let spc_aft = p_pr.and_then(|pp| first(pp, "spcAft"));
        let sa = spc_aft
            .and_then(|a| first(a, "spcPts"))
            .and_then(|s| attr(s, "val"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 100.0)
            .filter(|v| *v > 0.0);
        if sa.is_none() && spc_aft.and_then(|a| first(a, "spcPct")).is_some() {
            ctx.loss(LossKind::SpaceAfter, name);
        }
        let mt = p_pr
            .and_then(|pp| first(pp, "spcBef"))
            .and_then(|b| first(b, "spcPts"))
            .and_then(|s| attr(s, "val"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 100.0)
            .or(ctx.para_margin_top);
        let mut runs: Vec<RichRun> = Vec::new();
        for child in p.children.iter().filter_map(|n| n.as_element()) {
            match child.name.as_str() {
                "r" => {
                    let t = first(child, "t")
                        .and_then(|t| t.get_text())
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    let rpr = first(child, "rPr");
                    let st = RunStyle::from_rpr(rpr, slots, &para_defaults, &ctx.fonts);
                    if run_font_lossy(rpr, &para_defaults, &ctx.fonts) {
                        ctx.loss(LossKind::Font, name);
                    }
                    let click = rpr.and_then(|r| first(r, "hlinkClick"));
                    let link = click
                        .and_then(|h| attr(h, "id"))
                        .and_then(|rid| external_target(rels, rid));
                    if click.is_some() && link.is_none() {
                        ctx.loss(LossKind::Hyperlink, name);
                    }
                    runs.push(RichRun {
                        style: st,
                        text: t,
                        link,
                        is_math: false,
                    });
                }
                "br" => runs.push(RichRun {
                    style: None,
                    text: "\n".into(),
                    link: None,
                    is_math: false,
                }),
                "fld" => {
                    let t = first(child, "t")
                        .and_then(|t| t.get_text())
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    runs.push(RichRun {
                        style: None,
                        text: t,
                        link: None,
                        is_math: false,
                    });
                }
                "m" | "oMath" | "oMathPara" => {
                    if let Some(om) = extract_omath(child) {
                        let latex = crate::pptx::math::omml_to_latex(om);
                        runs.push(RichRun {
                            style: None,
                            text: format!("<latex>{latex}</latex>"),
                            link: None,
                            is_math: true,
                        });
                    }
                }
                "AlternateContent" => {
                    if let Some(choice) = first(child, "Choice") {
                        let m_el = first(choice, "m")
                            .or_else(|| first(choice, "oMath"))
                            .or_else(|| first(choice, "oMathPara"));
                        if let Some(om) = m_el.and_then(extract_omath) {
                            let latex = crate::pptx::math::omml_to_latex(om);
                            runs.push(RichRun {
                                style: None,
                                text: format!("<latex>{latex}</latex>"),
                                link: None,
                                is_math: true,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        if first(p, "endParaRPr").is_some_and(|mark| first(mark, "hlinkClick").is_some()) {
            ctx.loss(LossKind::Hyperlink, name);
        }
        if runs.is_empty() {
            runs.push(RichRun {
                style: None,
                text: String::new(),
                link: None,
                is_math: false,
            });
        }
        out_lines.push(RichLine {
            align: algn,
            line_height: ln,
            margin_top_px: mt,
            space_after_pt: sa,
            runs,
        });
    }

    // Drop trailing placeholder paragraphs.
    while out_lines
        .last()
        .is_some_and(|l| l.runs.iter().all(|r| r.text.is_empty()))
    {
        out_lines.pop();
    }
    if out_lines.is_empty() {
        return None;
    }

    let hoisted_space_after = (hoist_space_after
        && out_lines
            .iter()
            .all(|l| l.space_after_pt == out_lines[0].space_after_pt))
    .then_some(out_lines[0].space_after_pt)
    .flatten();
    if hoisted_space_after.is_some() {
        for line in &mut out_lines {
            line.space_after_pt = None;
        }
    }

    let first_line = &out_lines[0];
    let align = first_line.align;
    let line_height = first_line.line_height;
    let run_style = first_line.runs.first().and_then(|r| r.style.clone());
    if first_line.runs.iter().all(|r| r.text.is_empty()) && out_lines.len() == 1 {
        return None;
    }

    // Uniform path: one run per paragraph, identical style → plain text.
    // Force rich `<p>` markup whenever:
    //  * an empty spacer paragraph is present (plain-text splitting drops blank lines);
    //  * any non-math run contains `<` or `\(` (literal `<` / `\(` in plain text would
    //    be misinterpreted as rich-text tags or inline LaTeX on `pptd check` / `pptd build`).
    let has_empty_para = out_lines
        .iter()
        .any(|l| l.runs.iter().all(|r| r.text.is_empty()));
    let needs_rich_escape = out_lines.iter().any(|l| {
        l.runs
            .iter()
            .any(|r| !r.is_math && (r.text.contains('<') || r.text.contains("\\(")))
    });
    let uniform = !has_empty_para
        && !needs_rich_escape
        && out_lines.iter().all(|l| {
            l.runs.len() == 1
                && l.align == align
                && l.line_height == line_height
                && l.margin_top_px.is_none()
                && l.space_after_pt.is_none()
                && l.runs[0].style == run_style
                && l.runs[0].style.as_ref().and_then(|s| s.baseline).is_none()
                && l.runs[0].link.is_none()
        });

    let text = if uniform {
        out_lines
            .iter()
            .map(|l| l.runs[0].text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let mut markup = String::new();
        for l in &out_lines {
            let mut style_parts: Vec<String> = Vec::new();
            if let Some(a) = l.align {
                style_parts.push(format!("text-align:{}", css_align(a)));
            }
            if let Some(lh) = l.line_height {
                style_parts.push(format!("line-height:{lh}"));
            }
            if let Some(mt) = l.margin_top_px {
                style_parts.push(format!("margin-top:{mt}px"));
            }
            if let Some(sa) = l.space_after_pt {
                style_parts.push(format!("margin-bottom:{sa}px"));
            }
            let style_attr = if style_parts.is_empty() {
                String::new()
            } else {
                format!(" style=\"{}\"", style_parts.join("; "))
            };
            markup.push_str(&format!("<p{style_attr}>"));
            for run in &l.runs {
                let t = &run.text;
                let st = &run.style;
                let link = &run.link;
                if t.is_empty() {
                    continue;
                }
                if run.is_math {
                    markup.push_str(t);
                    continue;
                }
                if st == &run_style
                    && (st.is_none() || !t.contains('\n'))
                    && st.as_ref().and_then(|s| s.baseline).is_none()
                {
                    let escaped = escape_text(t);
                    markup.push_str(&match link {
                        Some(href) => format!("<a href=\"{}\">{escaped}</a>", escape_attr(href)),
                        None => escaped,
                    });
                    continue;
                }
                if t == "\n" {
                    markup.push(' ');
                    continue;
                }
                let mut span_style: Vec<String> = Vec::new();
                if let Some(sz) = st.as_ref().and_then(|s| s.font_size) {
                    span_style.push(format!("font-size:{sz}px"));
                }
                if let Some(c) = st.as_ref().and_then(|s| s.color.clone()) {
                    span_style.push(format!("color:{}", c.0));
                }
                if let Some(FontFamily::Single(n)) =
                    st.as_ref().and_then(|s| s.font_family.clone())
                {
                    span_style.push(format!("font-family:{n}"));
                }
                if st.as_ref().and_then(|s| s.bold) != run_style.as_ref().and_then(|s| s.bold) {
                    span_style.push(format!(
                        "font-weight:{}",
                        if st.as_ref().is_some_and(|s| s.bold == Some(true)) {
                            "bold"
                        } else {
                            "400"
                        }
                    ));
                }
                if st.as_ref().and_then(|s| s.italic) != run_style.as_ref().and_then(|s| s.italic) {
                    span_style.push(format!(
                        "font-style:{}",
                        if st.as_ref().is_some_and(|s| s.italic == Some(true)) {
                            "italic"
                        } else {
                            "normal"
                        }
                    ));
                }
                let baseline = st.as_ref().and_then(|s| s.baseline);
                if let Some(bl) = baseline.filter(|v| *v != 30000 && *v != -25000) {
                    span_style.push(format!("baseline:{bl}"));
                }
                let mut inner = escape_text(t);
                if !span_style.is_empty() {
                    inner = format!("<span style=\"{}\">{inner}</span>", span_style.join("; "));
                }
                if st.as_ref().is_some_and(|s| s.italic == Some(true)) {
                    inner = format!("<em>{inner}</em>");
                }
                if st.as_ref().is_some_and(|s| s.bold == Some(true)) {
                    inner = format!("<strong>{inner}</strong>");
                }
                match baseline {
                    Some(30000) => inner = format!("<sup>{inner}</sup>"),
                    Some(-25000) => inner = format!("<sub>{inner}</sub>"),
                    _ => {}
                }
                if let Some(href) = link {
                    inner = format!("<a href=\"{}\">{inner}</a>", escape_attr(href));
                }
                markup.push_str(&inner);
            }
            markup.push_str("</p>");
        }
        markup
    };
    if text.trim().is_empty() {
        return None;
    }

    Some(TextContent {
        text,
        style: None,
        color: run_style.as_ref().and_then(|s| s.color.clone()),
        font_size: run_style.as_ref().and_then(|s| s.font_size),
        font_family: run_style.as_ref().and_then(|s| s.font_family.clone()),
        bold: run_style.as_ref().and_then(|s| s.bold),
        italic: run_style.as_ref().and_then(|s| s.italic),
        background_color: None,
        line_height: if uniform { line_height } else { None },
        line_height_px: None,
        space_after: hoisted_space_after,
        letter_spacing: None,
        margin_top,
        margin_left,
        margin_right,
        margin_bottom,
        autofit,
        text_direction,
        wrap,
        body_pr_extras,
        align: match (align, anchor) {
            (Some(h), v) => Some(Alignment {
                horizontal: h,
                vertical: v.unwrap_or_default(),
            }),
            (None, Some(v)) => Some(Alignment {
                horizontal: HorizontalAlign::default(),
                vertical: v,
            }),
            (None, None) => None,
        },
        gradient: None,
        shadow: None,
        bullet_char: None,
        bullet_font: None,
        list_margin: None,
        list_indent: None,
    })
}

/// True when a run declares a Latin theme alias (`+mn-lt` / `+mj-lt`) that the
/// theme cannot resolve.
fn run_font_lossy(rpr: Option<&XmlEl>, defaults: &MasterDefaults, fonts: &ThemeFonts) -> bool {
    let Some(rpr) = rpr else {
        return false;
    };
    let explicit = |tag: &str| {
        first(rpr, tag)
            .and_then(|e| attr(e, "typeface"))
            .filter(|s| !s.is_empty())
    };
    let Some(tf) = explicit("latin").or(defaults.latin_typeface.as_deref()) else {
        return false;
    };
    tf.starts_with('+') && resolve_typeface(tf, fonts).is_none()
}

/// First run's text properties (others are compared for rich-text emission).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct RunStyle {
    pub(super) color: Option<Color>,
    pub(super) font_size: Option<f64>,
    pub(super) font_family: Option<FontFamily>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    /// `a:rPr@baseline` (1000ths of a percent): positive raises, negative
    /// lowers. `None` when the run sits on the normal baseline.
    pub(super) baseline: Option<i32>,
}

impl RunStyle {
    pub(super) fn from_rpr(
        rpr: Option<&XmlEl>,
        slots: &SlotColors,
        defaults: &MasterDefaults,
        fonts: &ThemeFonts,
    ) -> Option<Self> {
        let rpr = rpr?;
        let explicit_sz = attr(rpr, "sz").and_then(|s| s.parse::<f64>().ok());
        let explicit_fill = first(rpr, "solidFill").and_then(|sf| color_from_fill(sf, slots));
        let explicit_bold = xml_bool(rpr, "b");
        let explicit_italic = xml_bool(rpr, "i");
        let baseline = attr(rpr, "baseline")
            .and_then(|s| s.parse::<i32>().ok())
            .filter(|v| *v != 0);

        let font_size = explicit_sz.map(|s| s / 100.0).or(defaults.sz);
        let color = explicit_fill.or_else(|| defaults.color.clone());
        let bold = explicit_bold.or(defaults.bold);
        let italic = explicit_italic.or(defaults.italic);

        let latin: Option<String> = first(rpr, "latin")
            .and_then(|l| attr(l, "typeface"))
            .filter(|s| !s.is_empty())
            .or(defaults.latin_typeface.as_deref())
            .and_then(|tf| resolve_typeface(tf, fonts));
        let font_family = latin.map(FontFamily::Single);

        Some(RunStyle {
            color,
            font_size,
            font_family,
            bold,
            italic,
            baseline,
        })
    }
}
