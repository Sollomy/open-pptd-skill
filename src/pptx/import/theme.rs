//! Theme colors, master defaults, fills, gradients, shadows, and borders.

use std::collections::{BTreeMap, HashMap};

use xmltree::Element as XmlEl;

use crate::pptd::shared::{
    Border, Color, ColorStop, Fill, FontFamily, GradientFill, GradientType, LineStyle, Shadow,
};
use crate::pptd::theme::TextStyleConfig;

use super::opc::{attr, first, first_descendant, xml_bool};
use super::shapes::px;

/// clrScheme slot → hex color (already uppercase, no `#`).
pub(super) type SlotColors = BTreeMap<String, String>;

/// Theme major/minor font names, for resolving `+mj-lt` / `+mn-lt` aliases.
#[derive(Debug, Clone, Default)]
pub(super) struct ThemeFonts {
    pub(super) major: Option<String>,
    pub(super) minor: Option<String>,
}

/// Resolve a theme font alias to the concrete typeface, keeping explicit
/// names untouched. `+mn-lt`/`+mn-ea`/`+mn-cs` and the `+mj-*` set map to
/// the theme's minor/major Latin face.
pub(super) fn resolve_typeface(tf: &str, fonts: &ThemeFonts) -> Option<String> {
    match tf {
        "+mn-lt" | "+mn-ea" | "+mn-cs" => fonts.minor.clone(),
        "+mj-lt" | "+mj-ea" | "+mj-cs" => fonts.major.clone(),
        s => (!s.is_empty()).then(|| s.to_string()),
    }
}

pub(super) fn theme_fonts_from(theme: &XmlEl) -> ThemeFonts {
    let slot = |name: &str| {
        first_descendant(theme, name)
            .and_then(|f| first(f, "latin"))
            .and_then(|l| attr(l, "typeface"))
            .filter(|s| !s.is_empty() && !s.starts_with('+'))
            .map(str::to_string)
    };
    ThemeFonts {
        major: slot("majorFont"),
        minor: slot("minorFont"),
    }
}

/// Default run properties for plain (non-placeholder) text: the master's
/// `otherStyle → lvl1pPr → defRPr`. Runs lacking an explicit attribute
/// inherit these, and so does the paragraph mark — the rebuild must carry
/// them explicitly to reproduce the source (a missing `sz` in the source
/// run is *not* the renderer's 18pt default but the master's 28.35pt).
#[derive(Debug, Clone, Default)]
pub(super) struct MasterDefaults {
    pub(super) sz: Option<f64>,
    pub(super) color: Option<Color>,
    /// Raw typeface (may still be a `+mn-lt` alias; resolved at use).
    pub(super) latin_typeface: Option<String>,
    pub(super) bold: Option<bool>,
    pub(super) italic: Option<bool>,
    /// Inherited paragraph line spacing (fraction, `spcPct/100000`) from
    /// the placeholder's `lstStyle → lvl1pPr → lnSpc` — the slide paragraph
    /// inherits it when its own `<a:pPr>` carries no `<a:lnSpc>` (OOXML
    /// inheritance chain). `master_defaults` fills it from the master's
    /// `bodyStyle`; `placeholder_proto` may override it per placeholder
    /// with the layout's `lstStyle` value.
    pub(super) line_spacing: Option<f64>,
    /// Master `titleStyle → lvl1pPr → lnSpc` (fraction) for title
    /// placeholders, which inherit the title style rather than the body
    /// style.
    pub(super) title_line_spacing: Option<f64>,
    /// Inherited paragraph space-before (points, `spcBef`) from the
    /// placeholder chain — dropped when the slide paragraph omits
    /// `<a:spcBef>` and the master/layout supplies one (e.g. the standard
    /// body lvl1 `spcPts 1000`).
    pub(super) spc_bef: Option<f64>,
    /// Master `titleStyle → lvl1pPr → spcBef` (points).
    pub(super) title_spc_bef: Option<f64>,
}

pub(super) fn master_defaults(master: &XmlEl, slots: &SlotColors) -> MasterDefaults {
    let mut d = first_descendant(master, "otherStyle")
        .and_then(|other| first(other, "lvl1pPr"))
        .and_then(|lvl1| first(lvl1, "defRPr"))
        .map(|rpr| def_rpr_defaults(rpr, slots))
        .unwrap_or_default();

    // Capture the master's inherited paragraph line spacing and space-before
    // for placeholder text (`bodyStyle` / `titleStyle`), even when `otherStyle`
    // is absent. Each style resolves `spcPct` against its own `defRPr@sz`
    // (falling back to `d.sz`).
    let style_lvl1 =
        |style: &str| first_descendant(master, style).and_then(|s| first(s, "lvl1pPr"));
    let ln_spc = |lvl1: &XmlEl| {
        first(lvl1, "lnSpc")
            .and_then(|ls| first(ls, "spcPct"))
            .and_then(|sp| attr(sp, "val"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 100000.0)
    };
    let style_sz = |lvl1: &XmlEl| {
        first(lvl1, "defRPr")
            .and_then(|r| attr(r, "sz"))
            .and_then(|s| s.parse::<f64>().ok())
            .map(|v| v / 100.0)
            .or(d.sz)
    };

    if let Some(body_lvl1) = style_lvl1("bodyStyle") {
        d.line_spacing = ln_spc(body_lvl1);
        d.spc_bef = lvl1_spc_bef(body_lvl1, style_sz(body_lvl1));
    }
    if let Some(title_lvl1) = style_lvl1("titleStyle") {
        d.title_line_spacing = ln_spc(title_lvl1);
        d.title_spc_bef = lvl1_spc_bef(title_lvl1, style_sz(title_lvl1));
    }
    d
}

/// Master `lvl1pPr spcBef` in points: `spcPts` directly; `spcPct` resolved
/// against the style's default run size.
pub(super) fn lvl1_spc_bef(lvl1: &XmlEl, default_sz: Option<f64>) -> Option<f64> {
    let bef = first(lvl1, "spcBef")?;
    if let Some(pts) = first(bef, "spcPts").and_then(|s| attr(s, "val")) {
        return pts.parse::<f64>().ok().map(|v| v / 100.0);
    }
    let pct = first(bef, "spcPct")
        .and_then(|s| attr(s, "val"))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    // 0% → no space; a real percentage scales the default run size.
    Some(pct / 100000.0 * default_sz.unwrap_or(0.0))
}

/// `defRPr` → `MasterDefaults` (shared by the master `otherStyle` path and
/// the layout placeholder `lstStyle` path).
pub(super) fn def_rpr_defaults(rpr: &XmlEl, slots: &SlotColors) -> MasterDefaults {
    MasterDefaults {
        sz: attr(rpr, "sz")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| s / 100.0),
        color: first(rpr, "solidFill").and_then(|sf| color_from_fill(sf, slots)),
        latin_typeface: first(rpr, "latin")
            .and_then(|l| attr(l, "typeface"))
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        bold: xml_bool(rpr, "b"),
        italic: xml_bool(rpr, "i"),
        line_spacing: None,
        title_line_spacing: None,
        spc_bef: None,
        title_spc_bef: None,
    }
}

/// Capture the master's `txStyles` (title/body/other) first-level defaults
/// into PPTD `theme.textStyles` (`title`/`body`/`other` keys) so the
/// rebuild can re-emit the master's `txStyles`. Placeholder shapes inherit
/// these paragraph/run defaults per the OOXML chain, and the default run
/// size + line spacing drive placeholder line metrics in every consumer.
pub(super) fn master_text_styles(
    master: &XmlEl,
    slots: &SlotColors,
    fonts: &ThemeFonts,
) -> HashMap<String, TextStyleConfig> {
    let mut out = HashMap::new();
    let Some(tx_styles) = first(master, "txStyles") else {
        return out;
    };
    for (key, style_name) in [
        ("title", "titleStyle"),
        ("body", "bodyStyle"),
        ("other", "otherStyle"),
    ] {
        let Some(style) = first(tx_styles, style_name) else {
            continue;
        };
        let Some(lvl1) = first(style, "lvl1pPr") else {
            continue;
        };
        let rpr = first(lvl1, "defRPr");
        let cfg = TextStyleConfig {
            color: rpr
                .and_then(|r| first(r, "solidFill"))
                .and_then(|sf| color_from_fill(sf, slots)),
            font_size: rpr
                .and_then(|r| attr(r, "sz"))
                .and_then(|s| s.parse::<f64>().ok())
                .map(|v| v / 100.0),
            font_family: rpr
                .and_then(|r| first(r, "latin"))
                .and_then(|l| attr(l, "typeface"))
                .filter(|s| !s.is_empty())
                .and_then(|tf| resolve_typeface(tf, fonts))
                .map(FontFamily::Single),
            bold: rpr.and_then(|r| xml_bool(r, "b")),
            italic: rpr.and_then(|r| xml_bool(r, "i")),
            line_height: first(lvl1, "lnSpc")
                .and_then(|ls| first(ls, "spcPct"))
                .and_then(|sp| attr(sp, "val"))
                .and_then(|v| v.parse::<f64>().ok())
                .map(|v| v / 100000.0),
            margin_top: lvl1_spc_bef(
                lvl1,
                rpr.and_then(|r| attr(r, "sz"))
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|v| v / 100.0),
            ),
            background_color: None,
            line_height_px: None,
            letter_spacing: None,
        };
        let has_content = cfg.color.is_some()
            || cfg.font_size.is_some()
            || cfg.font_family.is_some()
            || cfg.bold.is_some()
            || cfg.italic.is_some()
            || cfg.line_height.is_some()
            || cfg.margin_top.is_some();
        if has_content {
            out.insert(key.to_string(), cfg);
        }
    }
    out
}

/// 12 clrScheme slots → hex strings, plus `bg1`/`tx1`/`bg2`/`tx2` resolved
/// independently through the master's `<p:clrMap>` (defaulting to
/// `lt1`/`dk1`/`lt2`/`dk2` when `<p:clrMap>` or an attribute is absent).
pub(super) fn read_theme_slots_from(theme: &XmlEl, master_el: Option<&XmlEl>) -> SlotColors {
    // clrScheme lives under a:themeElements in real themes.
    let Some(scheme) = first_descendant(theme, "clrScheme") else {
        return SlotColors::new();
    };
    let mut slots = SlotColors::new();
    for child in scheme.children.iter().filter_map(|n| n.as_element()) {
        if let Some(rgb) = color_rgb(child) {
            slots.insert(child.name.clone(), rgb);
        }
    }
    let clr_map = master_el.and_then(|m| first(m, "clrMap"));
    for (alias, default_target) in [
        ("bg1", "lt1"),
        ("tx1", "dk1"),
        ("bg2", "lt2"),
        ("tx2", "dk2"),
    ] {
        let target = clr_map
            .and_then(|cm| attr(cm, alias))
            .unwrap_or(default_target);
        if let Some(rgb) = slots.get(target).cloned() {
            slots.insert(alias.to_string(), rgb);
        }
    }
    slots
}

/// Master slide background (solid or gradient fill).
pub(super) fn read_master_bg(master: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    bg_fill(master, slots)
}

// ---------------------------------------------------------------------------
// Colors / fills / backgrounds / shadows / borders
// ---------------------------------------------------------------------------

/// `#RRGGBB` / `#RRGGBBAA` from a direct color child
/// (`srgbClr`/`schemeClr`/`sysClr`/`prstClr`) of a fill/reference element
/// (`solidFill`, `fontRef`, `fillRef`, `lnRef`, `bgRef`, `outerShdw`, `gs`, …).
/// Standard modifier children (`tint`/`shade`/`lumMod`/`lumOff`/`satMod`) are
/// folded into the final RGB.
pub(super) fn color_from_fill(el: &XmlEl, slots: &SlotColors) -> Option<Color> {
    if let Some(rgb) = first(el, "srgbClr") {
        let val = attr(rgb, "val")?;
        return Some(col_with_modifiers(val, alpha_of(rgb), rgb));
    }
    if let Some(scheme) = first(el, "schemeClr") {
        let slot = attr(scheme, "val")?;
        let rgb = slots.get(slot)?;
        return Some(col_with_modifiers(rgb, alpha_of(scheme), scheme));
    }
    if let Some(sys) = first(el, "sysClr") {
        let hex = match attr(sys, "lastClr") {
            Some(h) => h,
            None => match attr(sys, "val")? {
                "windowText" | "blackText" => "000000",
                "window" => "FFFFFF",
                _ => "000000",
            },
        };
        return Some(col_with_modifiers(hex, alpha_of(sys), sys));
    }
    if let Some(prst) = first(el, "prstClr") {
        let hex = preset_color_hex(attr(prst, "val")?);
        return Some(col_with_modifiers(hex, alpha_of(prst), prst));
    }
    None
}

/// OOXML `<a:prstClr val="..."/>` preset colour name → hex (no `#`).
/// Covers the named colours most commonly used in PowerPoint/WPS decks;
/// unknown names fall back to black so the run is never silently dropped
/// (the same value the master `otherStyle` would otherwise supply).
pub(super) fn preset_color_hex(name: &str) -> &'static str {
    match name {
        "black" | "dkBlack" => "000000",
        "white" | "ltWhite" => "FFFFFF",
        "red" => "FF0000",
        "green" => "008000",
        "lime" => "00FF00",
        "blue" => "0000FF",
        "yellow" => "FFFF00",
        "magenta" | "fuchsia" => "FF00FF",
        "cyan" | "aqua" => "00FFFF",
        "gray" | "grey" => "808080",
        "maroon" => "800000",
        "olive" => "808000",
        "navy" => "000080",
        "purple" => "800080",
        "teal" => "008080",
        "silver" => "C0C0C0",
        "orange" => "FFA500",
        "pink" => "FFC0CB",
        "brown" => "A52A2A",
        "gold" => "FFD700",
        "lightBlue" | "ltBlue" => "ADD8E6",
        "lightGreen" | "ltGreen" => "90EE90",
        "lightYellow" | "ltYellow" => "FFFFE0",
        "lightGray" | "ltGray" | "lightGrey" | "ltGrey" => "D3D3D3",
        "darkBlue" | "dkBlue" => "00008B",
        "darkGreen" | "dkGreen" => "006400",
        "darkRed" | "dkRed" => "8B0000",
        "darkGray" | "dkGray" | "darkGrey" | "dkGrey" => "A9A9A9",
        "darkCyan" | "dkCyan" => "008B8B",
        "darkOrange" | "dkOrange" => "FF8C00",
        "darkMagenta" | "dkMagenta" => "8B008B",
        "violet" => "EE82EE",
        "indigo" => "4B0082",
        "cornflowerBlue" => "6495ED",
        "chocolate" => "D2691E",
        "crimson" => "DC143C",
        "salmon" => "FA8072",
        _ => "000000",
    }
}

fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let r = (r / 255.0).clamp(0.0, 1.0);
    let g = (g / 255.0).clamp(0.0, 1.0);
    let b = (b / 255.0).clamp(0.0, 1.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d <= 1e-12 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() <= 1e-12 {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() <= 1e-12 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s <= 1e-12 {
        let v = l * 255.0;
        return (v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue_to_rgb = |mut t: f64| -> f64 {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (
        hue_to_rgb(h + 1.0 / 3.0) * 255.0,
        hue_to_rgb(h) * 255.0,
        hue_to_rgb(h - 1.0 / 3.0) * 255.0,
    )
}

/// Apply OOXML color modifiers (`tint`/`shade`/`lumMod`/`lumOff`/`satMod`)
/// in document order and compose the final `#RRGGBB` / `#RRGGBBAA`.
pub(super) fn col_with_modifiers(rgb: &str, alpha: Option<f64>, node: &XmlEl) -> Color {
    let hex = rgb.to_ascii_uppercase();
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return col(&hex, alpha);
    }
    let mut r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64;
    let mut g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f64;
    let mut b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f64;
    for m in node.children.iter().filter_map(|n| n.as_element()) {
        let Some(raw) = attr(m, "val").and_then(|v| v.parse::<f64>().ok()) else {
            continue;
        };
        match m.name.as_str() {
            "tint" => {
                let p = (raw / 100000.0).clamp(0.0, 1.0);
                r = r * p + 255.0 * (1.0 - p);
                g = g * p + 255.0 * (1.0 - p);
                b = b * p + 255.0 * (1.0 - p);
            }
            "shade" => {
                let p = (raw / 100000.0).clamp(0.0, 1.0);
                r *= p;
                g *= p;
                b *= p;
            }
            "lumMod" => {
                let p = (raw / 100000.0).clamp(0.0, 10.0);
                let (h, s, l) = rgb_to_hsl(r, g, b);
                (r, g, b) = hsl_to_rgb(h, s, (l * p).clamp(0.0, 1.0));
            }
            "lumOff" => {
                let p = (raw / 100000.0).clamp(-1.0, 1.0);
                let (h, s, l) = rgb_to_hsl(r, g, b);
                (r, g, b) = hsl_to_rgb(h, s, (l + p).clamp(0.0, 1.0));
            }
            "satMod" => {
                let p = (raw / 100000.0).clamp(0.0, 10.0);
                let (h, s, l) = rgb_to_hsl(r, g, b);
                (r, g, b) = hsl_to_rgb(h, (s * p).clamp(0.0, 1.0), l);
            }
            _ => {}
        }
    }
    let clamp = |v: f64| -> u8 { v.round().clamp(0.0, 255.0) as u8 };
    let out = format!("{:02X}{:02X}{:02X}", clamp(r), clamp(g), clamp(b));
    col(&out, alpha)
}

/// Direct color child of a theme `clrScheme` slot → uppercase `RRGGBB`.
fn color_rgb(el: &XmlEl) -> Option<String> {
    if let Some(srgb) = first(el, "srgbClr") {
        return attr(srgb, "val").map(|v| v.to_ascii_uppercase());
    }
    if let Some(sys) = first(el, "sysClr") {
        if let Some(last) = attr(sys, "lastClr") {
            return Some(last.to_ascii_uppercase());
        }
        return match attr(sys, "val")? {
            "windowText" | "blackText" => Some("000000".to_string()),
            "window" => Some("FFFFFF".to_string()),
            _ => None,
        };
    }
    None
}

fn alpha_of(el: &XmlEl) -> Option<f64> {
    first(el, "alpha")
        .and_then(|a| attr(a, "val"))
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v / 100000.0)
}

fn col(rgb: &str, alpha: Option<f64>) -> Color {
    match alpha {
        Some(a) if a < 0.999 => {
            // Below 100%: carry the alpha through #RRGGBBAA (resolve_color
            // round-trips it); fully transparent is not representable.
            let a8 = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color(format!("#{}{a8:02X}", rgb.to_ascii_uppercase()))
        }
        _ => Color(format!("#{}", rgb.to_ascii_uppercase())),
    }
}

pub(super) fn gradient_from(grad: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    let mut stops = Vec::new();
    for gs in first(grad, "gsLst")?
        .children
        .iter()
        .filter_map(|n| n.as_element())
    {
        if gs.name != "gs" {
            continue;
        }
        let pos = attr(gs, "pos")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 100000.0;
        let color = color_from_fill(gs, slots)?;
        stops.push(ColorStop {
            position: pos,
            color,
        });
    }
    if stops.is_empty() {
        return None;
    }
    if let Some(lin) = first(grad, "lin") {
        let angle = attr(lin, "ang")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|a| a / 60000.0);
        let scaled = xml_bool(lin, "scaled") == Some(true);
        Some(Fill::Gradient {
            gradient_type: GradientType::Linear,
            stops,
            angle,
            scaled,
        })
    } else if first(grad, "path").is_some() {
        Some(Fill::Gradient {
            gradient_type: GradientType::Radial,
            stops,
            angle: None,
            scaled: false,
        })
    } else {
        // ECMA-376 §20.1.8.33: a gradFill without <a:lin> or <a:path>
        // defaults to a 0° linear gradient.
        Some(Fill::Gradient {
            gradient_type: GradientType::Linear,
            stops,
            angle: Some(0.0),
            scaled: false,
        })
    }
}

/// Slide or master `<p:bg>` → background fill. `bgRef` resolves through the
/// theme; `bgPr` solid/gradient fills map directly; anything else → None.
pub(super) fn bg_fill(slide_or_master: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    let bg = first(slide_or_master, "cSld").and_then(|c| first(c, "bg"))?;
    if let Some(bg_pr) = first(bg, "bgPr") {
        return fill_and_border(bg_pr, slots);
    }
    if let Some(bg_ref) = first(bg, "bgRef") {
        return color_from_fill(bg_ref, slots).map(|color| Fill::Solid { color });
    }
    None
}

/// Fill from a property element (`bgPr` / `spPr`): solid / gradient;
/// `noFill`/absent → None.
pub(super) fn fill_and_border(sp_pr: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    if first(sp_pr, "noFill").is_some() {
        return None;
    }
    if let Some(solid) = first(sp_pr, "solidFill") {
        return color_from_fill(solid, slots).map(|color| Fill::Solid { color });
    }
    if let Some(grad) = first(sp_pr, "gradFill") {
        return gradient_from(grad, slots);
    }
    None
}

/// `<p:style><a:fillRef idx="N"> <colour/> </a:fillRef>` → the shape's fill
/// when `spPr` carries no explicit fill. `idx="0"` means noFill; `idx≥1`
/// uses the fillRef's child colour (resolved through the theme slots).
fn fill_ref_fill(sp_el: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    let style = first(sp_el, "style")?;
    let fr = first(style, "fillRef")?;
    if attr(fr, "idx")? == "0" {
        return None;
    }
    color_from_fill(fr, slots).map(|color| Fill::Solid { color })
}

/// Shape fill precedence: explicit `spPr` fill (solid/gradient) →
/// `<a:noFill/>`/`<a:grpFill/>`/`<a:blipFill/>`/`<a:pattFill/>` (do NOT fall
/// back to `fillRef`) → `<p:style><a:fillRef>`.
pub(super) fn shape_fill(sp_el: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    if let Some(sp_pr) = first(sp_el, "spPr") {
        if first(sp_pr, "noFill").is_some()
            || first(sp_pr, "grpFill").is_some()
            || first(sp_pr, "blipFill").is_some()
            || first(sp_pr, "pattFill").is_some()
        {
            return None;
        }
        if let Some(solid) = first(sp_pr, "solidFill") {
            return color_from_fill(solid, slots).map(|color| Fill::Solid { color });
        }
        if let Some(grad) = first(sp_pr, "gradFill") {
            return gradient_from(grad, slots);
        }
    }
    fill_ref_fill(sp_el, slots)
}

pub(super) fn solid_fill(sp_pr: &XmlEl, slots: &SlotColors) -> Option<Fill> {
    if first(sp_pr, "noFill").is_some() {
        return None;
    }
    first(sp_pr, "solidFill")
        .and_then(|f| color_from_fill(f, slots))
        .map(|color| Fill::Solid { color })
}

/// `<p:spPr><a:effectLst><a:outerShdw …>` → [`Shadow`]. The colour (incl.
/// `<a:alpha>`) is resolved via [`color_from_fill`]; `dist`/`dir` map to the
/// `[x, y]` offset (px); `blurRad` → `blur` (px).
pub(super) fn shadow_from(sp_pr: Option<&XmlEl>, slots: &SlotColors) -> Option<Shadow> {
    let sp_pr = sp_pr?;
    let effects = first(sp_pr, "effectLst")?;
    // Prefer outerShdw; fall back to innerShdw (renders inside the shape).
    let (shdw, inner) = match first(effects, "outerShdw") {
        Some(s) => (s, false),
        None => (first(effects, "innerShdw")?, true),
    };
    let blur = attr(shdw, "blurRad")
        .and_then(|s| s.parse::<f64>().ok())
        .map(px)
        .unwrap_or(0.0);
    let dist = attr(shdw, "dist")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let dir_deg = attr(shdw, "dir")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v / 60000.0)
        .unwrap_or(0.0);
    let r = dir_deg.to_radians();
    let ox = dist * r.cos();
    let oy = dist * r.sin();
    let offset = if ox.abs() < 1e-6 && oy.abs() < 1e-6 {
        None
    } else {
        Some((px(ox), px(oy)))
    };
    let color = color_from_fill(shdw, slots)?;
    let scale = attr(shdw, "sx")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v / 100000.0)
        .filter(|&v| (v - 1.0).abs() > 1e-6);
    let inner = if inner { Some(true) } else { None };
    Some(Shadow {
        blur,
        color,
        offset,
        scale,
        inner,
    })
}

/// Map OOXML `<a:prstDash val="…"/>` on a line element to PPTD [`LineStyle`].
pub(super) fn parse_dash_style(ln: &XmlEl) -> Option<LineStyle> {
    first(ln, "prstDash")
        .and_then(|d| attr(d, "val"))
        .map(|v| match v {
            "dash" | "sysDash" | "dashDot" | "lgDash" | "lgDashDot" | "lgDashDotDot"
            | "sysDashDot" | "sysDashDotDot" => LineStyle::Dash,
            "dot" => LineStyle::Dot,
            "sysDot" => LineStyle::SysDot,
            _ => LineStyle::Solid,
        })
}

/// Extract a gradient stroke from `<a:ln><a:gradFill>…`.
pub(super) fn line_gradient_fill(ln: &XmlEl, slots: &SlotColors) -> Option<GradientFill> {
    first(ln, "gradFill")
        .and_then(|g| gradient_from(g, slots))
        .and_then(|f| match f {
            Fill::Gradient {
                gradient_type,
                stops,
                angle,
                scaled,
            } => Some(GradientFill {
                gradient_type,
                stops,
                angle,
                scaled,
            }),
            _ => None,
        })
}

pub(super) fn border_from_el(sp_el: &XmlEl, slots: &SlotColors) -> Option<Border> {
    // Explicit `<a:ln>` in spPr takes precedence; `<a:noFill>` inside it
    // means "no line" and must NOT fall back to the style lnRef.
    if let Some(sp_pr) = first(sp_el, "spPr") {
        if let Some(ln) = first(sp_pr, "ln") {
            if first(ln, "noFill").is_some() {
                return None;
            }
            let width = attr(ln, "w")
                .and_then(|s| s.parse::<i64>().ok())
                .map(|v| px(v as f64));
            let style = parse_dash_style(ln);
            let gradient = line_gradient_fill(ln, slots);
            let color = first(ln, "solidFill").and_then(|s| color_from_fill(s, slots));
            let mut border = Border {
                style,
                width,
                color,
                gradient,
            };
            // A line with neither a solidFill nor a gradFill is colourised
            // by Office through `<p:style><a:lnRef …>`.
            if border.color.is_none() && border.gradient.is_none() {
                if let Some(ln_ref) = ln_ref_border(sp_el, slots) {
                    border.color = ln_ref.color;
                    border.width = border.width.or(ln_ref.width);
                    border.style = border.style.or(ln_ref.style);
                }
            }
            return Some(border);
        }
    }
    // No explicit spPr `<a:ln>` → fall back to `<p:style><a:lnRef>`.
    ln_ref_border(sp_el, slots)
}

/// `<p:style><a:lnRef idx="N"> <colour/> </a:lnRef>` → the shape's outline
/// when spPr carries no explicit `<a:ln>`. `idx="0"` → no line; `idx≥1`
/// references a theme line style.
fn ln_ref_border(sp_el: &XmlEl, slots: &SlotColors) -> Option<Border> {
    let style = first(sp_el, "style")?;
    let lr = first(style, "lnRef")?;
    let idx = attr(lr, "idx")?;
    if idx == "0" {
        return None;
    }
    let width = match idx {
        "1" => 0.5,
        "2" => 1.0,
        "3" => 1.5,
        _ => 1.0,
    };
    color_from_fill(lr, slots).map(|c| Border {
        style: Some(LineStyle::Solid),
        width: Some(width),
        color: Some(c),
        gradient: None,
    })
}
