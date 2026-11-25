//! Table (`a:tbl`) and native chart (`c:chart`) extraction from `<p:graphicFrame>`.

use std::collections::BTreeMap;

use xmltree::Element as XmlEl;

use crate::pptd::Chart;
use crate::pptd::elements::{Cell, CellMargin, Element, Table};
use crate::pptd::shared::{Alignment, Border, BorderSpec, Fill, VerticalAlign};

use super::opc::{attr, children, first, register_media, rels_part, xml_bool};
use super::shapes::{Xform, cnv_name, common, element_link, px};
use super::text::{body_pr_autofit, text_content};
use super::theme::{SlotColors, color_from_fill, line_gradient_fill, parse_dash_style};
use super::{EMU_PER_PX, LossKind, PageCtx};

/// One `a:lnL/lnR/lnT/lnB` cell border. Absent from `tcPr` → `None` (side not
/// specified); present with `noFill` → `Some(None)` (explicitly cleared).
fn cell_side_border(
    tc_pr: Option<&XmlEl>,
    tag: &str,
    slots: &SlotColors,
) -> Option<Option<Border>> {
    let ln = tc_pr.and_then(|p| first(p, tag))?;
    if first(ln, "noFill").is_some() {
        return Some(None);
    }
    Some(Some(Border {
        style: parse_dash_style(ln),
        width: attr(ln, "w").and_then(|w| w.parse::<f64>().ok()).map(px),
        color: first(ln, "solidFill").and_then(|f| color_from_fill(f, slots)),
        gradient: line_gradient_fill(ln, slots),
    }))
}

/// Assemble the PPTD `BorderSpec` from per-side results. All sides absent →
/// `None` (border field unset); all four identical → uniform; otherwise the
/// clockwise four-side form (a cleared/absent side becomes `null`).
fn cell_border_spec(sides: [Option<Option<Border>>; 4]) -> Option<BorderSpec> {
    if sides.iter().all(|s| s.is_none()) {
        return None;
    }
    let [l, r, t, b] = sides;
    let to_opt = |s: Option<Option<Border>>| s.flatten();
    let (l, r, t, b) = (to_opt(l), to_opt(r), to_opt(t), to_opt(b));
    if [&l, &r, &t, &b].iter().all(|s| s.is_some()) && l == r && r == t && t == b {
        return l.map(BorderSpec::Uniform);
    }
    Some(BorderSpec::Clockwise([t, r, b, l]))
}

/// `graphicFrame` carrying chart graphicData → PPTD `chart` element.
///
/// The chart XML, its `.rels` and the embedded workbook are copied into
/// `media/` verbatim; the model keeps only the part paths and a plot-kind
/// hint. The chart part is never rewritten — internal font/color references
/// stay exactly as authored.
pub(super) fn map_chart(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
) -> Option<Element> {
    let name = cnv_name(el).unwrap_or_else(|| "chart".into());
    let xfrm_el = first(el, "xfrm")?;
    let x = Xform::parse(xfrm_el).apply(group);

    let rid = first(el, "graphic")
        .and_then(|g| first(g, "graphicData"))
        .and_then(|d| first(d, "chart"))
        .and_then(|c| attr(c, "id"));
    let Some(part) = rid.and_then(|r| rels.get(r)) else {
        ctx.skip_loss(&name, "chart without a resolvable r:id", LossKind::Chart);
        return None;
    };
    let Some(info) = ctx.charts.get(part) else {
        ctx.skip_loss(
            &name,
            format!("chart part `{part}` is missing from the package"),
            LossKind::Chart,
        );
        return None;
    };

    let basename = register_media(ctx.media, part.clone());
    // Copy the chart's own `.rels` verbatim paired with `{basename}.rels` so
    // `ChartRegistry` finds it alongside `media/{basename}`.
    if info.workbook_part.is_some() {
        ctx.media
            .entry(rels_part(part))
            .or_insert_with(|| format!("{basename}.rels"));
    }
    let workbook = info.workbook_part.as_ref().map(|wb| {
        let wb_name = register_media(ctx.media, wb.clone());
        format!("media/{wb_name}")
    });

    let id = ctx.unique_id(&name, "chart");
    Some(Element::Chart(Box::new(Chart::passthrough(
        common(&x, id, element_link(el, rels)),
        format!("media/{basename}"),
        workbook,
        info.kind.clone(),
    ))))
}

/// `graphicFrame` carrying table graphicData → PPTD `table` element.
///
/// Merge reconstruction mirrors the forward writer: `hMerge` / `vMerge`
/// placeholder cells are dropped from the ragged `rows` arrays, while the
/// origin cell carries `rowSpan` / `colSpan`.
pub(super) fn map_table(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
) -> Option<Element> {
    let name = cnv_name(el).unwrap_or_else(|| "table".into());
    let xfrm_el = first(el, "xfrm")?;
    let x = Xform::parse(xfrm_el).apply(group);
    let tbl = first(el, "graphic")
        .and_then(|g| first(g, "graphicData"))
        .and_then(|d| first(d, "tbl"))?;

    let grid_cols: Vec<f64> = first(tbl, "tblGrid")
        .map(|g| {
            children(g, "gridCol")
                .iter()
                .filter_map(|c| attr(c, "w").and_then(|w| w.parse::<f64>().ok()).map(px))
                .collect()
        })
        .unwrap_or_default();
    let trs = children(tbl, "tr");
    if grid_cols.is_empty() || trs.is_empty() {
        ctx.skip(&name, "table without a grid or rows");
        return None;
    }
    let col_sum: f64 = grid_cols.iter().sum();
    let column_widths: Vec<f64> = if col_sum > 0.0 {
        grid_cols.iter().map(|w| w / col_sum).collect()
    } else {
        vec![1.0 / grid_cols.len() as f64; grid_cols.len()]
    };
    let raw_heights: Vec<f64> = trs
        .iter()
        .map(|tr| {
            attr(tr, "h")
                .and_then(|h| h.parse::<f64>().ok())
                .map(px)
                .unwrap_or(0.0)
        })
        .collect();
    let height_sum: f64 = raw_heights.iter().sum();
    let row_heights: Vec<f64> = if height_sum > 0.0 {
        raw_heights.iter().map(|h| h / height_sum).collect()
    } else {
        vec![1.0 / trs.len() as f64; trs.len()]
    };

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for tr in &trs {
        let mut row = Vec::new();
        for tc in children(tr, "tc") {
            let merged = |key: &str| xml_bool(tc, key) == Some(true);
            if merged("hMerge") || merged("vMerge") {
                continue;
            }
            let row_span = attr(tc, "rowSpan").and_then(|v| v.parse::<u32>().ok());
            let col_span = attr(tc, "gridSpan").and_then(|v| v.parse::<u32>().ok());
            let content = first(tc, "txBody")
                .and_then(|t| text_content(t, rels, ctx.slots, ctx, &name, false));
            let tc_pr = first(tc, "tcPr");
            let fill = tc_pr
                .and_then(|p| first(p, "solidFill"))
                .and_then(|f| color_from_fill(f, ctx.slots))
                .map(|c| Fill::Solid { color: c });
            let border = cell_border_spec([
                cell_side_border(tc_pr, "lnL", ctx.slots),
                cell_side_border(tc_pr, "lnR", ctx.slots),
                cell_side_border(tc_pr, "lnT", ctx.slots),
                cell_side_border(tc_pr, "lnB", ctx.slots),
            ]);
            let mut align = content.as_ref().and_then(|c| c.align);
            if let Some(anchor) = tc_pr.and_then(|p| attr(p, "anchor")) {
                let vertical = match anchor {
                    "t" => VerticalAlign::Top,
                    "b" => VerticalAlign::Bottom,
                    _ => VerticalAlign::Middle,
                };
                align = Some(Alignment {
                    horizontal: align.map(|a| a.horizontal).unwrap_or_default(),
                    vertical,
                });
            }
            let cell_margin = tc_pr.and_then(|p| {
                let pt = |key: &str| {
                    attr(p, key)
                        .and_then(|v| v.parse::<f64>().ok())
                        .map(|emu| emu / EMU_PER_PX)
                };
                let margin = CellMargin {
                    left: pt("marL"),
                    right: pt("marR"),
                    top: pt("marT"),
                    bottom: pt("marB"),
                };
                (margin != CellMargin::POWERPOINT_DEFAULT).then_some(margin)
            });
            let cell_body_pr = first(tc, "txBody").and_then(|t| first(t, "bodyPr"));
            let cell_autofit = cell_body_pr.and_then(|b| body_pr_autofit(Some(b)));
            if let Some(scale) = cell_body_pr.and_then(|b| first(b, "normAutofit")) {
                if attr(scale, "fontScale").is_some() || attr(scale, "lnSpcReduction").is_some() {
                    ctx.loss(LossKind::TableCellAutofit, &name);
                }
            }
            row.push(Cell {
                text: content.as_ref().map(|c| c.text.clone()),
                text_style: None,
                color: content.as_ref().and_then(|c| c.color.clone()),
                font_size: content.as_ref().and_then(|c| c.font_size),
                font_family: content.as_ref().and_then(|c| c.font_family.clone()),
                bold: content.as_ref().and_then(|c| c.bold),
                italic: content.as_ref().and_then(|c| c.italic),
                background_color: None,
                line_height: content.as_ref().and_then(|c| c.line_height),
                line_height_px: None,
                letter_spacing: None,
                margin_top: None,
                margin: cell_margin,
                autofit: cell_autofit,
                fill,
                border,
                align,
                row_span,
                col_span,
            });
        }
        rows.push(row);
    }

    let fill = first(tbl, "tblPr")
        .and_then(|p| first(p, "solidFill"))
        .and_then(|f| color_from_fill(f, ctx.slots))
        .map(|c| Fill::Solid { color: c });
    let id = ctx.unique_id(&name, "table");
    Some(Element::Table(Table {
        common: common(&x, id, element_link(el, rels)),
        column_widths,
        row_heights,
        rows,
        style: None,
        fill,
        shadow: None,
    }))
}
