//! Shapes, transforms (`a:xfrm`), custom path geometry (`GeomGuides`), pictures, media, lines, and icons.

use std::collections::BTreeMap;

use xmltree::Element as XmlEl;

use crate::Result;
use crate::pptd::elements::{
    ArrowType, Element, ElementCommon, Icon, Image, Line, Media, Shape, ShapeDef, Text, TextContent,
};
use crate::pptd::shared::{Alignment, Bounds, HorizontalAlign, ImageCrop, ImageFit, ImageFitMode};

use super::opc::{attr, children, external_target, first, register_media, xml_bool};
use super::text::{font_ref_defaults, lst_style_info, text_content};
use super::theme::{MasterDefaults, border_from_el, shadow_from, shape_fill, solid_fill};
use super::{
    EMU_PER_PX, LossKind, PPTD_ICON_URI, PageCtx, PlaceholderProto, lookup_placeholder, ph_key,
    placeholder_proto,
};

/// The shape's `a:xfrm` (optionally composed into an enclosing group box).
#[derive(Debug, Clone, Copy)]
pub(super) struct Xform {
    pub(super) off: (f64, f64),
    pub(super) ext: (f64, f64),
    pub(super) ch_off: (f64, f64),
    pub(super) ch_ext: (f64, f64),
    pub(super) rot: Option<f64>,
    pub(super) flip: Option<(bool, bool)>,
}

impl Xform {
    pub(super) fn parse(xfrm: &XmlEl) -> Self {
        // `a:off` uses x/y, `a:ext` uses cx/cy.
        let (off, ext) = match (xy(xfrm, "off", "x", "y"), xy(xfrm, "ext", "cx", "cy")) {
            (Some(off), Some(ext)) => (off, ext),
            _ => ((0.0, 0.0), (0.0, 0.0)),
        };
        let ch_off = xy(xfrm, "chOff", "x", "y").unwrap_or((0.0, 0.0));
        let ch_ext = xy(xfrm, "chExt", "cx", "cy").unwrap_or(ext);
        Xform {
            off,
            ext,
            ch_off,
            ch_ext,
            rot: attr(xfrm, "rot")
                .and_then(|s| s.parse().ok())
                .map(|v: i64| v as f64 / 60000.0),
            flip: Some((
                xml_bool(xfrm, "flipH") == Some(true),
                xml_bool(xfrm, "flipV") == Some(true),
            )),
        }
    }

    /// Compose this shape's xfrm with an enclosing flattened group xfrm.
    pub(super) fn apply(&self, group: Option<&Xform>) -> Xform {
        match group {
            None => *self,
            Some(g) => {
                let sx = if g.ch_ext.0 > 0.0 {
                    g.ext.0 / g.ch_ext.0
                } else {
                    1.0
                };
                let sy = if g.ch_ext.1 > 0.0 {
                    g.ext.1 / g.ch_ext.1
                } else {
                    1.0
                };
                let mut nx = g.off.0 + (self.off.0 - g.ch_off.0) * sx;
                let mut ny = g.off.1 + (self.off.1 - g.ch_off.1) * sy;
                let ncx = self.ext.0 * sx;
                let ncy = self.ext.1 * sy;

                let (g_fh, g_fv) = g.flip.unwrap_or((false, false));
                let (mut fh, mut fv) = self.flip.unwrap_or((false, false));
                if g_fh {
                    nx = g.off.0 + g.ext.0 - (nx - g.off.0) - ncx;
                    fh = !fh;
                }
                if g_fv {
                    ny = g.off.1 + g.ext.1 - (ny - g.off.1) - ncy;
                    fv = !fv;
                }
                let flip = if self.flip.is_some() || g_fh || g_fv {
                    Some((fh, fv))
                } else {
                    None
                };

                let mut rot = self.rot;
                if let Some(g_rot) = g.rot.filter(|r| r.abs() > 1e-6) {
                    let (sin, cos) = g_rot.to_radians().sin_cos();
                    let gcx = g.off.0 + g.ext.0 * 0.5;
                    let gcy = g.off.1 + g.ext.1 * 0.5;
                    let dx = (nx + ncx * 0.5) - gcx;
                    let dy = (ny + ncy * 0.5) - gcy;
                    let rcx = gcx + dx * cos - dy * sin;
                    let rcy = gcy + dx * sin + dy * cos;
                    nx = rcx - ncx * 0.5;
                    ny = rcy - ncy * 0.5;
                    rot = Some(self.rot.unwrap_or(0.0) + g_rot);
                }

                Xform {
                    off: (nx, ny),
                    ext: (ncx, ncy),
                    ch_off: self.ch_off,
                    ch_ext: self.ch_ext,
                    rot,
                    flip,
                }
            }
        }
    }
}

fn xy(el: &XmlEl, name: &str, x_attr: &str, y_attr: &str) -> Option<(f64, f64)> {
    let n = first(el, name)?;
    Some((
        attr(n, x_attr).and_then(|s| s.parse().ok()).unwrap_or(0.0),
        attr(n, y_attr).and_then(|s| s.parse().ok()).unwrap_or(0.0),
    ))
}

pub(super) fn px(emu: f64) -> f64 {
    emu / EMU_PER_PX
}

pub(super) fn to_bounds(x: &Xform) -> Bounds {
    Bounds {
        x: px(x.off.0),
        y: px(x.off.1),
        width: px(x.ext.0),
        height: px(x.ext.1),
    }
}

pub(super) fn common(x: &Xform, id: String, link: Option<String>) -> ElementCommon {
    ElementCommon {
        element_id: id,
        bounds: to_bounds(x),
        rotation: x.rot,
        opacity: None,
        flip: x.flip.filter(|(h, v)| *h || *v),
        group_id: None,
        group_bounds: None,
        link,
    }
}

/// External click target on an element's `p:cNvPr` (`a:hlinkClick r:id`).
pub(super) fn element_link(el: &XmlEl, rels: &BTreeMap<String, String>) -> Option<String> {
    let nv = el
        .children
        .iter()
        .filter_map(|n| n.as_element())
        .find(|c| c.name.starts_with("nv") && c.name.ends_with("Pr"))?;
    let cnv = first(nv, "cNvPr")?;
    let rid = attr(first(cnv, "hlinkClick")?, "id")?;
    external_target(rels, rid)
}

/// The first `p:cNvPr` name of an spTree child.
pub(super) fn cnv_name(el: &XmlEl) -> Option<String> {
    let nv = el
        .children
        .iter()
        .filter_map(|n| n.as_element())
        .find(|c| c.name.starts_with("nv") && c.name.ends_with("Pr"))?;
    first(nv, "cNvPr")
        .and_then(|p| attr(p, "name"))
        .map(str::to_string)
}

/// `graphicFrame` → graphicData URI.
pub(super) fn graphic_uri(el: &XmlEl) -> Option<&str> {
    let graphic = first(el, "graphic")?;
    let data = first(graphic, "graphicData")?;
    attr(data, "uri")
}

// ---------------------------------------------------------------------------
// Text boxes and shapes (`p:sp`)
// ---------------------------------------------------------------------------

pub(super) fn map_sp(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
    in_layout: bool,
) -> Result<Option<Element>> {
    let name = cnv_name(el).unwrap_or_else(|| "sp".to_string());
    let nv_pr = first(el, "nvSpPr").and_then(|nv| first(nv, "nvPr"));
    let ph = nv_pr.and_then(|p| first(p, "ph"));

    // Capture layout/master placeholder protos for the OOXML inheritance chain,
    // merging layout overrides onto any matching master proto (so a layout
    // placeholder that omits `<a:xfrm>` reuses the master's transform).
    if in_layout {
        if let Some(ph_el) = ph {
            let key = ph_key(ph_el);
            let existing = lookup_placeholder(&ctx.layout_placeholders, &key).cloned();
            if let Some(proto) = placeholder_proto(el, ctx.slots, existing.as_ref()) {
                ctx.layout_placeholders.insert(key, proto);
            }
            if ctx.in_master {
                return Ok(None);
            }
        }
    }

    let ext = nv_pr
        .and_then(|p| first(p, "extLst"))
        .and_then(|lst| {
            children(lst, "ext")
                .into_iter()
                .find(|e| attr(e, "uri") == Some(PPTD_ICON_URI))
        })
        .and_then(|ext| first(ext, "icon"));
    let sp_pr = first(el, "spPr");
    let xfrm_el = sp_pr.and_then(|p| first(p, "xfrm"));

    let mut inherited_defaults: Option<MasterDefaults> = None;
    let inherited_proto: Option<PlaceholderProto> =
        ph.and_then(|ph_el| lookup_placeholder(&ctx.layout_placeholders, &ph_key(ph_el)).cloned());
    if let Some(proto) = &inherited_proto {
        inherited_defaults = Some(proto.defaults.clone());
    }
    let proto_anchor = inherited_proto.as_ref().and_then(|p| p.anchor);
    let (x, inherited_prst) = match (xfrm_el, inherited_proto) {
        (Some(xel), _) => (Xform::parse(xel).apply(group), None),
        (None, Some(proto)) => (proto.xfrm, proto.prst),
        (None, None) => {
            if ph.is_some() {
                ctx.skip_loss(
                    &name,
                    "placeholder has no a:xfrm and no matching layout placeholder",
                    LossKind::Placeholder,
                );
            } else {
                ctx.skip(&name, "shape has no a:xfrm");
            }
            return Ok(None);
        }
    };

    // `pptd:icon` extension → icon element.
    if let Some(icon_el) = ext {
        if let Some(icon_name) = icon_payload(icon_el) {
            let id = ctx.unique_id(&name, "icon");
            let fill = sp_pr.and_then(|p| solid_fill(p, ctx.slots));
            return Ok(Some(Element::Icon(Icon {
                common: common(&x, id, element_link(el, rels)),
                icon_name,
                fill,
                border: border_from_el(el, ctx.slots),
                shadow: shadow_from(sp_pr, ctx.slots),
            })));
        }
    }

    let geom = sp_pr.and_then(|p| first(p, "custGeom"));
    let prst = sp_pr
        .and_then(|p| first(p, "prstGeom"))
        .and_then(|g| attr(g, "prst"))
        .map(str::to_string)
        .or(inherited_prst);

    // Text boxes: a `p:txBody` with non-empty text.
    if let Some(tx_body) = first(el, "txBody") {
        let mut base = ctx.defaults.clone();
        base = font_ref_defaults(base, el, ctx.slots);
        if let Some(d) = &inherited_defaults {
            base = d.clone();
        }
        let (base, bullet) = lst_style_info(base, tx_body, ctx.slots);
        let para_line_spacing = if ph.is_some() {
            inherited_defaults
                .as_ref()
                .and_then(|d| d.line_spacing)
                .or_else(|| {
                    if ph
                        .as_ref()
                        .and_then(|p| attr(p, "type"))
                        .is_some_and(|t| t == "title")
                    {
                        ctx.defaults.title_line_spacing.or(ctx.master_line_spacing)
                    } else {
                        ctx.master_line_spacing
                    }
                })
        } else {
            None
        };
        let para_margin_top = if ph.is_some() {
            inherited_defaults
                .as_ref()
                .and_then(|d| d.spc_bef)
                .or_else(|| {
                    if ph
                        .as_ref()
                        .and_then(|p| attr(p, "type"))
                        .is_some_and(|t| t == "title")
                    {
                        ctx.defaults.title_spc_bef.or(ctx.master_spc_bef)
                    } else {
                        ctx.master_spc_bef
                    }
                })
        } else {
            None
        };
        let saved_defaults = ctx.defaults.clone();
        ctx.defaults = base;
        let saved_para = ctx.para_line_spacing;
        ctx.para_line_spacing = para_line_spacing;
        let saved_mtop = ctx.para_margin_top;
        ctx.para_margin_top = para_margin_top;
        let mut content = text_content(tx_body, rels, ctx.slots, ctx, &name, true);
        ctx.para_margin_top = saved_mtop;
        ctx.para_line_spacing = saved_para;
        ctx.defaults = saved_defaults;

        let body_anchor_explicit = first(el, "txBody")
            .and_then(|tb| first(tb, "bodyPr"))
            .and_then(|bp| attr(bp, "anchor"))
            .is_some();
        if let (Some(anchor), Some(c)) = (proto_anchor, content.as_mut()) {
            if !body_anchor_explicit {
                match c.align.as_mut() {
                    Some(a) => a.vertical = anchor,
                    None => {
                        c.align = Some(Alignment {
                            horizontal: HorizontalAlign::default(),
                            vertical: anchor,
                        });
                    }
                }
            }
        }
        if let Some(ref mut c) = content {
            c.bullet_char = bullet.char;
            c.bullet_font = bullet.font;
            c.list_margin = bullet.margin;
            c.list_indent = bullet.indent;
        }
        if let Some(content) = content {
            let id = ctx.unique_id(&name, "text");
            let fill = shape_fill(el, ctx.slots);
            let border = border_from_el(el, ctx.slots);
            let placeholder = ph
                .as_ref()
                .map(|p| attr(p, "type").unwrap_or("body").to_string());
            return Ok(Some(Element::Text(Box::new(Text {
                common: common(&x, id, element_link(el, rels)),
                content,
                fill,
                border,
                placeholder,
            }))));
        }
        if ph.is_some() {
            let id = ctx.unique_id(&name, "text");
            let placeholder = ph
                .and_then(|p| attr(p, "type"))
                .unwrap_or("body")
                .to_string();
            return Ok(Some(Element::Text(Box::new(Text {
                common: common(&x, id, element_link(el, rels)),
                content: TextContent::default(),
                fill: None,
                border: None,
                placeholder: Some(placeholder),
            }))));
        }
    }

    if sp_pr.is_some_and(|p| first(p, "blipFill").is_some()) {
        ctx.skip(&name, "image fills on shapes are not representable in PPTD");
        return Ok(None);
    }
    let fill = shape_fill(el, ctx.slots);

    let mut element = None;
    if let Some(geom_el) = geom {
        match custom_shape(geom_el, x.ext) {
            Ok(Some((view_box, path, adjustments))) => {
                let id = ctx.unique_id(&name, "custom shape");
                element = Some(Element::Shape(Shape {
                    common: common(&x, id, element_link(el, rels)),
                    shape_name: "custom".to_string(),
                    adjustments,
                    view_box: Some(view_box),
                    path: Some(path),
                    fill,
                    border: border_from_el(el, ctx.slots),
                    shadow: shadow_from(sp_pr, ctx.slots),
                }));
            }
            Ok(None) => {}
            Err(reason) => ctx.skip(&name, reason),
        }
    } else if let Some(prst) = prst {
        let id = ctx.unique_id(&name, "shape");
        let adjustments = av_list_values(sp_pr.and_then(|p| first(p, "prstGeom")));
        element = Some(Element::Shape(Shape {
            common: common(&x, id, element_link(el, rels)),
            shape_name: prst,
            adjustments,
            view_box: None,
            path: None,
            fill,
            border: border_from_el(el, ctx.slots),
            shadow: shadow_from(sp_pr, ctx.slots),
        }));
    } else {
        ctx.skip(&name, "shape with neither prstGeom nor custGeom");
    }

    // Drop invisible leftovers (no fill, no border, no shadow, no text).
    if let Some(Element::Shape(shape)) = &element {
        if shape.fill.is_none() && shape.border.is_none() && shape.shadow.is_none() {
            return Ok(None);
        }
    }
    Ok(element)
}

/// `prstGeom > avLst` guide values (preset units, e.g. 16667 = 1/6 radius).
fn av_list_values(prst: Option<&XmlEl>) -> Option<Vec<f64>> {
    let mut values = Vec::new();
    for gd in prst?.children.iter().filter_map(|n| n.as_element()) {
        if gd.name != "avLst" {
            continue;
        }
        for g in gd.children.iter().filter_map(|n| n.as_element()) {
            if g.name == "gd" {
                if let Some(fmla) = attr(g, "fmla") {
                    if let Some(rest) = fmla.strip_prefix("val ") {
                        if let Ok(v) = rest.parse::<f64>() {
                            values.push(v);
                        }
                    }
                }
            }
        }
    }
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

// ---------------------------------------------------------------------------
// Custom geometry: guide evaluation + path → SVG string
// ---------------------------------------------------------------------------

/// Simple geometry-guide evaluator (ECMA-376 `CT_GeomGuide`, §20.1.9.11).
pub(super) struct GeomGuides {
    values: BTreeMap<String, f64>,
}

impl GeomGuides {
    pub(super) fn new(rect: (f64, f64, f64, f64), av: &[(String, f64)]) -> Self {
        let (l, t, r, b) = rect;
        let w = r - l;
        let h = b - t;
        let mut values = BTreeMap::new();
        values.insert("l".into(), l);
        values.insert("t".into(), t);
        values.insert("r".into(), r);
        values.insert("b".into(), b);
        values.insert("w".into(), w);
        values.insert("h".into(), h);
        values.insert("hc".into(), (l + r) / 2.0);
        values.insert("vc".into(), (t + b) / 2.0);
        values.insert("wd2".into(), w / 2.0);
        values.insert("hd2".into(), h / 2.0);
        for (name, v) in av {
            values.insert(name.clone(), *v);
        }
        Self { values }
    }

    pub(super) fn get(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    /// Evaluate one `fmla` string; returns `Err` when an operator is unknown,
    /// has bad arity, divides by zero, or references an unresolved operand.
    pub(super) fn evaluate(&mut self, name: &str, fmla: &str) -> std::result::Result<(), String> {
        let tokens: Vec<&str> = fmla.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(format!("empty formula for guide `{name}`"));
        }
        let op = tokens[0];
        let resolve = |tok: &str, g: &GeomGuides| -> Option<f64> {
            tok.parse::<f64>().ok().or_else(|| g.get(tok))
        };
        let args: Vec<Option<f64>> = tokens[1..].iter().map(|t| resolve(t, self)).collect();
        let value = match op {
            "val" => args.first().copied().flatten(),
            "*/" => {
                let &[Some(a), Some(b), Some(c)] = args.as_slice() else {
                    return Err(format!("bad */ fmla `{fmla}`"));
                };
                if c == 0.0 {
                    return Err(format!("division by zero in */ fmla `{fmla}`"));
                }
                Some(a * b / c)
            }
            "+/" => {
                let &[Some(a), Some(b), Some(c)] = args.as_slice() else {
                    return Err(format!("bad +/ fmla `{fmla}`"));
                };
                if c == 0.0 {
                    return Err(format!("division by zero in +/ fmla `{fmla}`"));
                }
                Some((a + b) / c)
            }
            "+-" => {
                let &[Some(a), Some(b), Some(c)] = args.as_slice() else {
                    return Err(format!("bad +- fmla `{fmla}`"));
                };
                Some(a + b - c)
            }
            "--" => {
                let &[Some(a), Some(b), Some(c)] = args.as_slice() else {
                    return Err(format!("bad -- fmla `{fmla}`"));
                };
                Some(a - b - c)
            }
            "min" => {
                let &[Some(a), Some(b)] = args.as_slice() else {
                    return Err(format!("bad min fmla `{fmla}`"));
                };
                Some(a.min(b))
            }
            "max" => {
                let &[Some(a), Some(b)] = args.as_slice() else {
                    return Err(format!("bad max fmla `{fmla}`"));
                };
                Some(a.max(b))
            }
            "abs" => args.first().copied().flatten().map(f64::abs),
            "sqrt" => args.first().copied().flatten().map(f64::sqrt),
            "sin" | "cos" | "tan" => {
                // ECMA-376 §20.1.9.11: `sin x y` = x * sin(y), y in 60000ths of a degree.
                let &[Some(x), Some(y)] = args.as_slice() else {
                    return Err(format!("bad {op} fmla `{fmla}`"));
                };
                let rad = (y / 60000.0).to_radians();
                let f = match op {
                    "sin" => rad.sin(),
                    "cos" => rad.cos(),
                    _ => rad.tan(),
                };
                Some(x * f)
            }
            "at2" => {
                // ECMA-376 §20.1.9.11: `at2 x y` = atan2(y, x) in 60000ths of a degree.
                let &[Some(x), Some(y)] = args.as_slice() else {
                    return Err(format!("bad at2 fmla `{fmla}`"));
                };
                Some(y.atan2(x).to_degrees() * 60000.0)
            }
            "cat2" => {
                // ECMA-376 §20.1.9.11: `cat2 x y z` = x * cos(atan2(z, y)).
                let &[Some(x), Some(y), Some(z)] = args.as_slice() else {
                    return Err(format!("bad cat2 fmla `{fmla}`"));
                };
                Some(x * z.atan2(y).cos())
            }
            "sat2" => {
                // ECMA-376 §20.1.9.11: `sat2 x y z` = x * sin(atan2(z, y)).
                let &[Some(x), Some(y), Some(z)] = args.as_slice() else {
                    return Err(format!("bad sat2 fmla `{fmla}`"));
                };
                Some(x * z.atan2(y).sin())
            }
            _ => return Err(format!("unsupported guide operator `{op}` in `{fmla}`")),
        };
        match value {
            Some(v) => {
                self.values.insert(name.to_string(), v);
                Ok(())
            }
            None => Err(format!("guide `{name}` has unresolved operands: `{fmla}`")),
        }
    }
}

/// Outcome of evaluating a `custGeom`: view box, SVG path string, and any
/// preset adjustments.
pub(super) type CustomGeom = ((f64, f64), String, Option<Vec<f64>>);

/// Extract `<a:path>` list from a `custGeom` → PPTD `(view_box, path, adjustments)`.
/// Returns `Ok(None)` for silently-skippable empty geometry.
pub(super) fn custom_shape(
    geom: &XmlEl,
    shape_ext: (f64, f64),
) -> std::result::Result<Option<CustomGeom>, String> {
    let path_lst = first(geom, "pathLst").ok_or("custGeom without pathLst")?;
    let paths = children(path_lst, "path");
    let Some(first_path) = paths.first() else {
        return Ok(None);
    };

    // Seed guides from the first `<a:path>` coordinate space (falling back to
    // the shape's `a:ext` in EMU), never from symbolic `<a:rect l="l" ...>`.
    let default_w = if shape_ext.0 > 0.0 {
        shape_ext.0
    } else {
        100.0
    };
    let default_h = if shape_ext.1 > 0.0 {
        shape_ext.1
    } else {
        100.0
    };
    let seed_w = attr(first_path, "w")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(default_w);
    let seed_h = attr(first_path, "h")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(default_h);
    let rect = (0.0, 0.0, seed_w, seed_h);

    // Adjustment values from avLst feed `adj` guides (their fmla is `val N`).
    let mut av: Vec<(String, f64)> = Vec::new();
    if let Some(av_lst) = first(geom, "avLst") {
        for gd in children(av_lst, "gd") {
            if let Some(fmla) = attr(gd, "fmla") {
                let gname = attr(gd, "name").unwrap_or("adj").to_string();
                if let Some(rest) = fmla.strip_prefix("val ") {
                    if let Ok(v) = rest.parse::<f64>() {
                        av.push((gname, v));
                    }
                }
            }
        }
    }
    let mut guides = GeomGuides::new(rect, &av);

    // Evaluate gdLst to a fixed point so forward references resolve without
    // retrying unresolvable formulas futilely.
    let mut pending: Vec<(String, String)> = first(geom, "gdLst")
        .into_iter()
        .flat_map(|lst| children(lst, "gd"))
        .map(|gd| {
            (
                attr(gd, "name").unwrap_or("").to_string(),
                attr(gd, "fmla").unwrap_or("").to_string(),
            )
        })
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut next = Vec::new();
        let mut last_err = String::new();
        for (gname, fmla) in pending {
            if let Err(err) = guides.evaluate(&gname, &fmla) {
                last_err = err;
                next.push((gname, fmla));
            }
        }
        if next.len() == before {
            return Err(format!("could not evaluate guide(s): {last_err}"));
        }
        pending = next;
    }

    // Build the SVG path string from every `<a:path>`.
    let mut d_parts: Vec<String> = Vec::new();
    let mut view_box: Option<(f64, f64)> = None;
    for p in paths {
        let w = attr(p, "w")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(seed_w);
        let h = attr(p, "h")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(seed_h);
        if let Some((prev_w, prev_h)) = view_box {
            if (prev_w - w).abs() > 1e-6 || (prev_h - h).abs() > 1e-6 {
                return Err(format!(
                    "custGeom mixes incompatible path coordinate spaces ({prev_w}x{prev_h} vs {w}x{h})"
                ));
            }
        } else {
            view_box = Some((w, h));
        }

        let mut d = String::new();
        for seg in p.children.iter().filter_map(|n| n.as_element()) {
            let pt_opt = |s: &XmlEl| pt(s, &guides).ok();
            let seg_pt = |s: &XmlEl| -> Option<(f64, f64)> {
                let child = first(s, "pt")?;
                pt(child, &guides).ok()
            };
            match seg.name.as_str() {
                "moveTo" => {
                    if let Some((x, y)) = seg_pt(seg) {
                        d.push_str(&format!("M {} {}", rnd(x), rnd(y)));
                    }
                }
                "lnTo" => {
                    if let Some((x, y)) = seg_pt(seg) {
                        d.push_str(&format!("L {} {}", rnd(x), rnd(y)));
                    }
                }
                "cubicBezTo" => {
                    let pts = seg
                        .children
                        .iter()
                        .filter_map(|n| n.as_element())
                        .filter_map(pt_opt)
                        .collect::<Vec<_>>();
                    if pts.len() == 3 {
                        d.push_str(&format!(
                            "C {} {} {} {} {} {}",
                            rnd(pts[0].0),
                            rnd(pts[0].1),
                            rnd(pts[1].0),
                            rnd(pts[1].1),
                            rnd(pts[2].0),
                            rnd(pts[2].1)
                        ));
                    }
                }
                "quadBezTo" => {
                    let pts = seg
                        .children
                        .iter()
                        .filter_map(|n| n.as_element())
                        .filter_map(pt_opt)
                        .collect::<Vec<_>>();
                    if pts.len() == 2 {
                        d.push_str(&format!(
                            "Q {} {} {} {}",
                            rnd(pts[0].0),
                            rnd(pts[0].1),
                            rnd(pts[1].0),
                            rnd(pts[1].1)
                        ));
                    }
                }
                "arcTo" => return Err("arcTo not supported".into()),
                "close" => d.push('Z'),
                _ => {}
            }
        }
        if !d.is_empty() {
            d_parts.push(d);
        }
    }
    let Some(view_box) = view_box else {
        return Ok(None);
    };
    if d_parts.is_empty() {
        return Ok(None);
    }
    let path = d_parts.join(" ");
    Ok(Some((view_box, path, None)))
}

fn rnd(v: f64) -> i64 {
    v.round() as i64
}

fn pt(el: &XmlEl, guides: &GeomGuides) -> std::result::Result<(f64, f64), String> {
    let x_raw = attr(el, "x").map(str::to_string);
    let y_raw = attr(el, "y").map(str::to_string);
    let gx = x_raw
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| guides.get(x_raw.as_deref()?));
    let gy = y_raw
        .as_deref()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| guides.get(y_raw.as_deref()?));
    match (gx, gy) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(format!(
            "path point references an unknown guide (x={x_raw:?} y={y_raw:?})"
        )),
    }
}

// ---------------------------------------------------------------------------
// Pictures / media / connectors / icons
// ---------------------------------------------------------------------------

pub(super) fn map_pic(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
) -> Option<Element> {
    let name = cnv_name(el).unwrap_or_else(|| "pic".to_string());
    let blip_fill = first(el, "blipFill");
    let embed = blip_fill
        .and_then(|b| first(b, "blip"))
        .and_then(|b| attr(b, "embed"));
    let sp_pr = first(el, "spPr");
    let xfrm = sp_pr.and_then(|p| first(p, "xfrm"))?;
    let x = Xform::parse(xfrm).apply(group);

    let nv_pr = first(el, "nvPicPr").and_then(|n| first(n, "nvPr"));
    let av = nv_pr.and_then(|n| {
        first(n, "videoFile")
            .map(|v| ("video", v))
            .or_else(|| first(n, "audioFile").map(|a| ("audio", a)))
    });
    if let Some((kind, av_el)) = av {
        return map_media(el, kind, av_el, rels, ctx, name, x);
    }

    let src = match embed.and_then(|rid| rels.get(rid)) {
        Some(target) => {
            let ext = target.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif") {
                ctx.skip_loss(
                    &name,
                    format!("media extension .{ext} is not supported by the builder"),
                    LossKind::Media,
                );
                return None;
            }
            let basename = register_media(ctx.media, target.clone());
            format!("media/{basename}")
        }
        None => {
            ctx.skip(&name, "picture without r:embed");
            return None;
        }
    };

    let (fit, crop) = src_rect_fit(blip_fill);
    let id = ctx.unique_id(&name, "image");
    let crop_shape = sp_pr
        .and_then(|p| first(p, "prstGeom"))
        .and_then(|g| attr(g, "prst"))
        .filter(|prst| *prst != "rect" && *prst != "custom")
        .map(|prst| ShapeDef {
            shape_name: prst.to_string(),
            adjustments: None,
            view_box: None,
            path: None,
        });
    let soft_edge = sp_pr
        .and_then(|p| first(p, "effectLst"))
        .and_then(|e| first(e, "softEdge"))
        .and_then(|s| attr(s, "rad"))
        .and_then(|v| v.parse::<f64>().ok())
        .map(|emu| emu / EMU_PER_PX);
    Some(Element::Image(Image {
        common: common(&x, id, element_link(el, rels)),
        src,
        crop_shape,
        fit,
        crop,
        border: None,
        shadow: None,
        soft_edge,
    }))
}

/// Map a media `p:pic` (`a:videoFile`/`a:audioFile` in `nvPr`) to a [`Media`]
/// element: the clip from `r:link`, the poster frame from `r:embed`.
fn map_media(
    el: &XmlEl,
    kind: &str,
    av_el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    name: String,
    x: Xform,
) -> Option<Element> {
    let link = attr(av_el, "link").and_then(|rid| rels.get(rid));
    let Some(target) = link else {
        ctx.skip(&name, "media clip without r:link target");
        return None;
    };
    let ext = target.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "mp4" | "m4v" | "mov" | "mp3" | "m4a" | "wav" | "wma"
    ) {
        ctx.skip_loss(
            &name,
            format!("media clip extension .{ext} is not supported"),
            LossKind::Media,
        );
        return None;
    }
    let basename = register_media(ctx.media, target.clone());

    let poster = first(el, "blipFill")
        .and_then(|b| first(b, "blip"))
        .and_then(|b| attr(b, "embed"))
        .and_then(|rid| rels.get(rid))
        .cloned()
        .map(|part| {
            let poster_name = register_media(ctx.media, part);
            format!("media/{poster_name}")
        });

    let id = ctx.unique_id(&name, kind);
    let media = Media {
        common: common(&x, id, element_link(el, rels)),
        src: format!("media/{basename}"),
        poster,
    };
    Some(if kind == "video" {
        Element::Video(Box::new(media))
    } else {
        Element::Audio(Box::new(media))
    })
}

/// Map `a:srcRect` ppm values to PPTD fit/crop.
fn src_rect_fit(blip_fill: Option<&XmlEl>) -> (Option<ImageFit>, Option<ImageCrop>) {
    let Some(rect) = blip_fill.and_then(|b| first(b, "srcRect")) else {
        return (
            Some(ImageFit {
                mode: ImageFitMode::Fill,
            }),
            None,
        );
    };
    let v = |k: &str| {
        attr(rect, k)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let (l, t, r, b) = (v("l"), v("t"), v("r"), v("b"));
    let f = |n: f64| n / 100000.0;
    if l != 0.0 || t != 0.0 || r != 0.0 || b != 0.0 {
        if (t < 0.0 || b < 0.0) && !(l > 0.0 || r > 0.0) {
            (
                Some(ImageFit {
                    mode: ImageFitMode::Contain,
                }),
                None,
            )
        } else {
            let crop = Some(ImageCrop {
                left: Some(f(l)),
                top: Some(f(t)),
                right: Some(f(r)),
                bottom: Some(f(b)),
            });
            (
                Some(ImageFit {
                    mode: ImageFitMode::Fill,
                }),
                crop,
            )
        }
    } else {
        (
            Some(ImageFit {
                mode: ImageFitMode::Fill,
            }),
            None,
        )
    }
}

fn parse_arrow_type(el: &XmlEl) -> Option<ArrowType> {
    match attr(el, "type")? {
        "triangle" | "arrow" => Some(ArrowType::Arrow),
        "stealth" => Some(ArrowType::Stealth),
        "diamond" => Some(ArrowType::Diamond),
        "oval" => Some(ArrowType::Oval),
        // "none" and unknown arrowhead kinds map to None.
        _ => None,
    }
}

fn line_arrows(sp_pr: Option<&XmlEl>) -> Option<(Option<ArrowType>, Option<ArrowType>)> {
    let ln = first(sp_pr?, "ln")?;
    let head = first(ln, "headEnd").and_then(parse_arrow_type);
    let tail = first(ln, "tailEnd").and_then(parse_arrow_type);
    if head.is_some() || tail.is_some() {
        Some((head, tail))
    } else {
        None
    }
}

/// Straight connector → `elementType: line` (degenerate axis allowed).
pub(super) fn map_line(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
) -> Option<Element> {
    let name = cnv_name(el).unwrap_or_else(|| "line".to_string());
    let sp_pr = first(el, "spPr");
    let xfrm = sp_pr.and_then(|p| first(p, "xfrm"))?;
    let x = Xform::parse(xfrm).apply(group);

    // The `line` preset runs corner-to-corner; flips select the corners.
    let (mut x0, mut y0) = (0.0f64, 0.0f64);
    let (mut x1, mut y1) = (x.ext.0, x.ext.1);
    if let Some((fh, fv)) = x.flip {
        if fh {
            std::mem::swap(&mut x0, &mut x1);
        }
        if fv {
            std::mem::swap(&mut y0, &mut y1);
        }
    }
    let view_box = (x.ext.0, x.ext.1);
    let points = format!("{},{} {},{}", rnd(x0), rnd(y0), rnd(x1), rnd(y1));

    let border = border_from_el(el, ctx.slots);
    let arrow = line_arrows(sp_pr);
    let id = ctx.unique_id(&name, "line");
    let mut cmn = common(&x, id, element_link(el, rels));
    // Flips are already baked into `points`; keeping `flip` would double-flip on rebuild.
    cmn.flip = None;
    Some(Element::Line(Line {
        common: cmn,
        view_box,
        points,
        curve: None,
        arrow,
        border,
        shadow: None,
    }))
}

/// Decode the base64url `pptd:icon` JSON payload → `iconName`.
fn icon_payload(icon_el: &XmlEl) -> Option<String> {
    let text = icon_el.get_text()?;
    let bytes = base64url_decode(text.trim())?;
    let val: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    val.get("iconName")?.as_str().map(str::to_string)
}

fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut bits: u32 = 0;
    let mut nbits = 0u32;
    for ch in s.bytes() {
        let v = TABLE.iter().position(|&c| c == ch)?;
        bits = (bits << 6) | v as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((bits >> nbits) as u8);
        }
    }
    Some(out)
}
