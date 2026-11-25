//! PPTX → PPTD reverse compiler (`pptd convert`).
//!
//! Reads an existing OPC `.pptx` package via `zip` + `xmltree` and produces a
//! PPTD project directory that — rebuilt with the forward pipeline — renders
//! the same slides for the supported element corpus:
//!
//! * text boxes (`p:sp` + `p:txBody`), shapes (`p:sp` without text) with
//!   preset geometry or custom path geometry (guide formulas are evaluated at
//!   import time and baked into the SVG path);
//! * straight connectors (`p:cxnSp`) → `elementType: line`;
//! * pictures (`p:pic`) with `media/…` assets copied next to the project;
//! * native charts (`p:graphicFrame` + `c:chart`): the chart XML, its
//!   embedded workbook and both parts' bytes are carried verbatim into
//!   `media/` and re-emitted by the builder;
//! * `pptd:icon` extension shapes → `elementType: icon` (round-trips our own
//!   files and Kimi exports);
//! * solid / gradient fills, borders, theme colors, page backgrounds.
//!
//! Constructs the downstream pipeline cannot represent are **not silently
//! dropped**: they are reported in [`ConvertReport::skipped`] with the slide
//! number, element name and reason (embedded OLE objects, image fills,
//! unusual geometry or transforms…).
//!
//! [`crate::hash`] bookkeeping is built in: `convert` compares the source
//! `.pptx` hash against the `.sync.hash` records in the output directory and
//! skips work when they match; a completed conversion writes a fresh hash.
//! Fidelity contract: element geometry is converted with the exact inverse of
//! the writer's px↔EMU mapping (`12700` EMU per px), so rebuilding a converted
//! project reproduces the original XML coordinates to the EMU.

mod opc;
mod shapes;
mod table;
mod text;
mod theme;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use xmltree::Element as XmlEl;

use crate::hash;
use crate::pptd::ast::{Page, Presentation};
use crate::pptd::elements::{Element, GroupDef, GroupXfrm};
use crate::pptd::layout::{LayoutDef, PlaceholderDef};
use crate::pptd::shared::{Color, Fill, FontFamily, VerticalAlign};
use crate::pptd::theme::{TextStyleConfig, Theme};
use crate::{Error, Result};

use self::opc::{
    MAX_MEDIA_BYTES, MAX_XML_BYTES, attr, children, first, first_rels_target, layout_rels,
    parse_part, parse_size, parse_slide_rels, parse_xml, presentation_slides, read_part_bytes,
    read_title, rels_targets, root_presentation_part,
};
use self::shapes::{Xform, cnv_name, graphic_uri, map_line, map_pic, map_sp, px, to_bounds};
use self::table::{map_chart, map_table};
use self::theme::{
    MasterDefaults, SlotColors, ThemeFonts, bg_fill, color_from_fill, def_rpr_defaults,
    gradient_from, lvl1_spc_bef, master_defaults, master_text_styles, read_master_bg,
    read_theme_slots_from, resolve_typeface, theme_fonts_from,
};

/// EMU per design px; the exact inverse of [`super::render::emu`].
const EMU_PER_PX: f64 = 12700.0;

/// Extension URI of the `pptd:icon` round-trip marker.
const PPTD_ICON_URI: &str = "{F5677B7D-0D2A-4D9B-9A5C-6D8D7D0E5A5D}";

/// Semantic color key → clrScheme slot (must mirror `pptx::theme`).
const SEMANTIC_KEYS: [(&str, &str); 12] = [
    ("text", "dk1"),
    ("text2", "dk2"),
    ("background", "lt1"),
    ("background2", "lt2"),
    ("primary", "accent1"),
    ("secondary", "accent2"),
    ("accent", "accent3"),
    ("success", "accent4"),
    ("warning", "accent5"),
    ("danger", "accent6"),
    ("link", "hlink"),
    ("folHlink", "folHlink"),
];

/// One unsupported construct that was reported instead of silently dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    /// One-based slide number; `0` for layout/master content.
    pub page: usize,
    /// Element name from `cNvPr`.
    pub name: String,
    /// Why it could not be represented in PPTD.
    pub reason: String,
    /// Loss category this skip was also aggregated into (so the CLI can
    /// report one summary line per category instead of one line per shape).
    pub loss: Option<LossKind>,
}

/// A mapping that necessarily loses source information.
///
/// Unlike [`Skipped`] (an element the pipeline cannot represent at all),
/// a loss is a style or attribute that is dropped while the element itself
/// survives. Each variant names one such mapping; the report aggregates
/// them so a regression is visible without diffing every `.page` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LossKind {
    /// Native `<c:chart>` whose part could not be carried over.
    Chart,
    /// Hyperlink target that cannot be expressed (internal/action links) or
    /// a paragraph-end click marker with no PPTD representation.
    Hyperlink,
    /// Media outside the PNG/JPEG/GIF corpus the builder can package.
    Media,
    /// Font face that was replaced by a fallback or dropped by slate merging.
    Font,
    /// Table-cell inset reset to the PowerPoint default.
    TableCellMargin,
    /// Table-cell autofit scaling (`normAutofit` attributes) dropped.
    TableCellAutofit,
    /// Run-level baseline (superscript/subscript) flattened.
    Baseline,
    /// Paragraph space-after (`a:spcAft`) dropped, e.g. percentage spacing.
    SpaceAfter,
    /// Placeholder with neither `<a:xfrm>` nor a matching layout placeholder.
    Placeholder,
}

impl LossKind {
    /// Stable machine-readable code used by `--json`.
    pub fn code(self) -> &'static str {
        match self {
            LossKind::Chart => "chart",
            LossKind::Hyperlink => "hyperlink",
            LossKind::Media => "media",
            LossKind::Font => "font",
            LossKind::TableCellMargin => "table_cell_margin",
            LossKind::TableCellAutofit => "table_cell_autofit",
            LossKind::Baseline => "baseline",
            LossKind::SpaceAfter => "space_after",
            LossKind::Placeholder => "placeholder",
        }
    }

    /// Human-readable category name.
    pub fn label(self) -> &'static str {
        match self {
            LossKind::Chart => "native chart",
            LossKind::Hyperlink => "rich-text hyperlink",
            LossKind::Media => "unsupported media",
            LossKind::Font => "font face",
            LossKind::TableCellMargin => "table cell padding",
            LossKind::TableCellAutofit => "table cell autofit",
            LossKind::Baseline => "run baseline",
            LossKind::SpaceAfter => "paragraph space-after",
            LossKind::Placeholder => "unresolved placeholder",
        }
    }

    /// One-line explanation of what the category means for the rebuilt deck.
    pub fn detail(self) -> &'static str {
        match self {
            LossKind::Chart => "chart part missing from the source package",
            LossKind::Hyperlink => {
                "link target is internal/action-based, or the click sits on a paragraph-end marker"
            }
            LossKind::Media => "builder packages PNG/JPEG/GIF only; file not copied",
            LossKind::Font => {
                "font alias could not be resolved, so the run falls back to the default face"
            }
            LossKind::TableCellMargin => "cell insets could not be carried into the PPTD model",
            LossKind::TableCellAutofit => "normAutofit fontScale/lnSpcReduction has no PPTD field",
            LossKind::Baseline => "run shift has no representation",
            LossKind::SpaceAfter => "only absolute spcPts is representable, not spcPct",
            LossKind::Placeholder => "placeholder has no transform and no matching layout entry",
        }
    }
}

/// Aggregated loss for one [`LossKind`], with the first occurrence as the
/// locator (`page: 0` means layout/master content).
#[derive(Debug, Clone, PartialEq)]
pub struct Loss {
    pub kind: LossKind,
    pub count: usize,
    pub page: usize,
    pub element: String,
}

/// Accumulates losses across slides and layouts.
#[derive(Debug, Default)]
struct LossTracker {
    entries: BTreeMap<LossKind, Loss>,
}

impl LossTracker {
    fn record(&mut self, kind: LossKind, page: usize, element: &str) {
        self.entries
            .entry(kind)
            .and_modify(|loss| loss.count += 1)
            .or_insert_with(|| Loss {
                kind,
                count: 1,
                page,
                element: element.to_string(),
            });
    }

    fn into_vec(self) -> Vec<Loss> {
        self.entries.into_values().collect()
    }
}

/// Result of a conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertReport {
    /// Where the PPTD project was written.
    pub output_dir: PathBuf,
    /// Number of slides converted.
    pub page_count: usize,
    /// Total elements written across all pages.
    pub element_count: usize,
    /// Media files copied into `media/`.
    pub media_count: usize,
    /// Unsupported constructs (honest failures, never silent).
    pub skipped: Vec<Skipped>,
    /// Lossy mappings aggregated per [`LossKind`], ordered by category.
    /// Empty means every supported construct round-tripped losslessly.
    pub losses: Vec<Loss>,
    /// SHA-256 of the source `.pptx`, also recorded in the `.sync.hash`
    /// sidecar. `None` only when a stored hash matched and nothing was
    /// re-converted (a fresh sync point re-emits the hash anyway).
    pub src_hash: Option<String>,
    /// SHA-256 matched the stored record before this call: the source
    /// is already in sync, so **no conversion was performed**. Callers must
    /// not overwrite `work_dir` after a skipped convert.
    pub skipped_unchanged: bool,
    /// True when legacy sidecars (`.src.hash`/`.build.hash`, including the
    /// old hash-only format) were migrated into `.sync.hash` during this
    /// call. Path-less records cannot authorize `build`'s overwrite guard,
    /// so they are repaired here.
    pub migrated_sidecars: bool,
}

/// Parsed slideMaster + linked theme state, cached once per master part.
struct MasterBundle {
    xml: XmlEl,
    rels: BTreeMap<String, String>,
    slots: SlotColors,
    fonts: ThemeFonts,
    defaults: MasterDefaults,
    styles: HashMap<String, TextStyleConfig>,
    bg: Option<Fill>,
}

fn load_master_bundle<'a>(
    zip: &mut zip::ZipArchive<fs::File>,
    master_part: &str,
    theme_cache: &mut HashMap<String, XmlEl>,
    master_cache: &'a mut HashMap<String, MasterBundle>,
) -> Option<&'a MasterBundle> {
    if !master_cache.contains_key(master_part) {
        let master_xml = parse_part(zip, master_part).ok()?;
        let rels = layout_rels(zip, master_part);
        let theme_part = first_rels_target(zip, master_part, "theme");
        let (slots, fonts) = match theme_part {
            Some(tp) => {
                if !theme_cache.contains_key(&tp) {
                    if let Ok(theme_el) = parse_part(zip, &tp) {
                        theme_cache.insert(tp.clone(), theme_el);
                    }
                }
                match theme_cache.get(&tp) {
                    Some(theme_el) => (
                        read_theme_slots_from(theme_el, Some(&master_xml)),
                        theme_fonts_from(theme_el),
                    ),
                    None => (SlotColors::new(), ThemeFonts::default()),
                }
            }
            None => (SlotColors::new(), ThemeFonts::default()),
        };
        let defaults = master_defaults(&master_xml, &slots);
        let styles = master_text_styles(&master_xml, &slots, &fonts);
        let bg = read_master_bg(&master_xml, &slots);
        master_cache.insert(
            master_part.to_string(),
            MasterBundle {
                xml: master_xml,
                rels,
                slots,
                fonts,
                defaults,
                styles,
                bg,
            },
        );
    }
    master_cache.get(master_part)
}

/// Convert a `.pptx` package into a PPTD project directory.
///
/// The source hash is checked against the `.sync.hash` records in `out_dir`
/// first: if it matches the last sync point, this is a no-op (returns a
/// report with [`ConvertReport::skipped_unchanged`] set) and the existing
/// PPTD project is left untouched.
pub fn convert_pptx_to_pptd(input: &Path, out_dir: &Path) -> Result<ConvertReport> {
    let mut records = hash::Records::load(out_dir);
    let mut migrated = records.has_legacy();
    let src_hash = hash::sha256_of(input)?;

    if records.get(input).is_none() {
        if let Some(legacy) = records.legacy_hash() {
            if *legacy == src_hash {
                records.set(input, &src_hash);
                migrated = true;
            }
        }
    }

    if records.get(input) == Some(&src_hash) {
        if migrated {
            records.save(out_dir)?;
        }
        return Ok(ConvertReport {
            output_dir: out_dir.to_path_buf(),
            page_count: 0,
            element_count: 0,
            media_count: 0,
            skipped: Vec::new(),
            losses: Vec::new(),
            src_hash: None,
            skipped_unchanged: true,
            migrated_sidecars: migrated,
        });
    }

    let file = fs::File::open(input)
        .map_err(|e| Error::Invalid(format!("cannot open {}: {e}", input.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| Error::Invalid(format!("not a valid OPC zip ({}): {e}", input.display())))?;

    let presentation_part = root_presentation_part(&mut zip)?;
    let pres_bytes = read_part_bytes(&mut zip, &presentation_part, MAX_XML_BYTES)?;
    let pres_raw = String::from_utf8_lossy(&pres_bytes);
    let pres = parse_xml(pres_bytes.as_slice())
        .map_err(|e| Error::Invalid(format!("parse XML in {presentation_part}: {e}")))?;

    let size = parse_size(&pres)?;
    let mut theme_cache: HashMap<String, XmlEl> = HashMap::new();
    let mut master_cache: HashMap<String, MasterBundle> = HashMap::new();

    let primary_master = first_rels_target(&mut zip, &presentation_part, "slideMaster");
    let (slots, fonts, defaults, master_styles, master_bg) = match primary_master.as_deref() {
        Some(master) => match load_master_bundle(&mut zip, master, &mut theme_cache, &mut master_cache) {
            Some(bundle) => (
                bundle.slots.clone(),
                bundle.fonts.clone(),
                bundle.defaults.clone(),
                bundle.styles.clone(),
                bundle.bg.clone(),
            ),
            None => (
                SlotColors::new(),
                ThemeFonts::default(),
                MasterDefaults::default(),
                HashMap::new(),
                None,
            ),
        },
        None => (
            SlotColors::new(),
            ThemeFonts::default(),
            MasterDefaults::default(),
            HashMap::new(),
            None,
        ),
    };

    // Slides in `sldIdLst` presentation order (not archive order).
    let slide_parts = presentation_slides(&pres, &pres_raw, &mut zip, &presentation_part)?;
    let chart_info = scan_chart_parts(&mut zip);

    fs::create_dir_all(out_dir.join("pages"))
        .map_err(|e| Error::Invalid(format!("create pages dir: {e}")))?;

    let mut media: BTreeMap<String, String> = BTreeMap::new(); // part path -> basename
    let mut pages = Vec::with_capacity(slide_parts.len());
    let mut skipped = Vec::new();
    let mut losses = LossTracker::default();
    let mut element_count = 0usize;
    let mut layouts: HashMap<String, LayoutDef> = HashMap::new();
    let mut layout_cache: HashMap<String, LayoutBuild> = HashMap::new();
    let mut used_layout_keys: BTreeSet<String> = BTreeSet::new();

    {
        let master_parts = rels_targets(&mut zip, &presentation_part, "slideMaster");
        let mut master_layout_parts: Vec<String> = Vec::new();
        for master in &master_parts {
            for lp in rels_targets(&mut zip, master, "slideLayout") {
                if !master_layout_parts.contains(&lp) {
                    master_layout_parts.push(lp);
                }
            }
        }
        let referenced: BTreeSet<String> = slide_parts
            .iter()
            .filter_map(|s| first_rels_target(&mut zip, s, "slideLayout"))
            .collect();
        let mut ordered: Vec<&String> = master_layout_parts
            .iter()
            .filter(|p| referenced.contains(*p))
            .collect();
        ordered.extend(
            master_layout_parts
                .iter()
                .filter(|p| !referenced.contains(*p)),
        );
        for part in ordered {
            let lp = part.as_str();
            if layout_cache.contains_key(lp) {
                continue;
            }
            let master_of_layout = first_rels_target(&mut zip, lp, "slideMaster");
            let built = build_layout(
                &mut zip,
                lp,
                master_of_layout.as_deref(),
                &slots,
                &mut theme_cache,
                &mut master_cache,
                &chart_info,
                &mut media,
                &mut skipped,
                &mut losses,
                &mut used_layout_keys,
            )?;
            layouts.insert(built.key.clone(), built.def.clone());
            layout_cache.insert(lp.to_string(), built);
        }
    }

    for (idx, part) in slide_parts.iter().enumerate() {
        let page_no = idx + 1;
        let slide = parse_part(&mut zip, part)?;
        let (rels, layout_part, notes_part) = parse_slide_rels(&mut zip, part);
        let notes = notes_part
            .as_deref()
            .and_then(|np| extract_notes(&mut zip, np));

        let (layout_field, lprotos, lslots, ldefaults, lfonts, lfbg) = match layout_part.as_deref() {
            Some(lp) => {
                if !layout_cache.contains_key(lp) {
                    let master_of_layout = first_rels_target(&mut zip, lp, "slideMaster");
                    let built = build_layout(
                        &mut zip,
                        lp,
                        master_of_layout.as_deref(),
                        &slots,
                        &mut theme_cache,
                        &mut master_cache,
                        &chart_info,
                        &mut media,
                        &mut skipped,
                        &mut losses,
                        &mut used_layout_keys,
                    )?;
                    layouts.insert(built.key.clone(), built.def.clone());
                    layout_cache.insert(lp.to_string(), built);
                }
                let lb = layout_cache.get(lp).expect("just inserted");
                (
                    Some(lb.key.clone()),
                    lb.protos.clone(),
                    lb.slots.clone(),
                    lb.defaults.clone(),
                    lb.fonts.clone(),
                    lb.fallback_bg.clone(),
                )
            }
            None => (
                None,
                BTreeMap::new(),
                slots.clone(),
                defaults.clone(),
                fonts.clone(),
                master_bg.clone(),
            ),
        };
        let master_line_spacing = ldefaults.line_spacing;
        let master_spc_bef = ldefaults.spc_bef;
        let mut ctx = PageCtx {
            page_no,
            slots: &lslots,
            defaults: ldefaults,
            master_line_spacing,
            master_spc_bef,
            para_line_spacing: None,
            para_margin_top: None,
            layout_placeholders: lprotos,
            fonts: lfonts,
            used_ids: BTreeSet::new(),
            media: &mut media,
            skipped: &mut skipped,
            losses: &mut losses,
            charts: &chart_info,
            fallback_bg: lfbg,
            layout_key: layout_field.clone(),
            groups: HashMap::new(),
            group_seq: 0,
            in_master: false,
        };
        let page = extract_slide(&slide, part, &mut ctx, &rels, notes)?;
        element_count += page.elements.len();

        let file_name = format!("{page_no}.page");
        let page_path = out_dir.join("pages").join(&file_name);
        let yaml = serde_yaml::to_string(&page)
            .map_err(|e| Error::Invalid(format!("serialize page {page_no}: {e}")))?;
        fs::write(&page_path, yaml)
            .map_err(|e| Error::Invalid(format!("write {}: {e}", page_path.display())))?;
        pages.push(format!("pages/{file_name}"));
    }

    // Copy referenced media parts once (bounded against ZIP bombs).
    let media_dir = out_dir.join("media");
    let mut media_count = 0usize;
    for (part_path, basename) in &media {
        let data = read_part_bytes(&mut zip, part_path, MAX_MEDIA_BYTES)?;
        fs::create_dir_all(&media_dir)
            .map_err(|e| Error::Invalid(format!("create media dir: {e}")))?;
        fs::write(media_dir.join(basename), data)
            .map_err(|e| Error::Invalid(format!("write media/{basename}: {e}")))?;
        media_count += 1;
    }

    let title = read_title(&mut zip)?;
    let theme = if slots.is_empty() {
        None
    } else {
        let mut colors = HashMap::new();
        for (key, slot) in SEMANTIC_KEYS {
            if let Some(hex) = slots.get(slot) {
                colors.insert(key.to_string(), Color(format!("#{hex}")));
            }
        }
        Some(Theme {
            colors,
            text_styles: master_styles,
            table_styles: HashMap::new(),
        })
    };

    let presentation = Presentation {
        version: "v2".to_string(),
        title,
        author: None,
        description: None,
        custom_fonts: Vec::new(),
        size,
        safe_fonts: false,
        theme,
        layouts: if layouts.is_empty() {
            None
        } else {
            Some(layouts)
        },
        pages,
    };
    let deck_yaml = serde_yaml::to_string(&presentation)
        .map_err(|e| Error::Invalid(format!("serialize deck: {e}")))?;
    fs::write(out_dir.join("deck.pptd"), deck_yaml)
        .map_err(|e| Error::Invalid(format!("write deck.pptd: {e}")))?;

    records.set(input, &src_hash);
    records.save(out_dir)?;

    Ok(ConvertReport {
        output_dir: out_dir.to_path_buf(),
        page_count: slide_parts.len(),
        element_count,
        media_count,
        skipped,
        losses: losses.into_vec(),
        src_hash: Some(src_hash),
        skipped_unchanged: false,
        migrated_sidecars: migrated,
    })
}

/// Native chart parts discovered in the source package: plot-family hint and
/// the embedded workbook the chart's `<c:externalData>` points at.
#[derive(Debug, Clone, Default)]
struct ChartInfo {
    kind: Option<String>,
    workbook_part: Option<String>,
}

/// Discover every chart definition part in the source package.
fn scan_chart_parts(zip: &mut zip::ZipArchive<fs::File>) -> HashMap<String, ChartInfo> {
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_owned()))
        .filter(|n| {
            n.starts_with("ppt/charts/") && n.ends_with(".xml") && !n.contains("/_rels/")
        })
        .collect();
    let mut out = HashMap::new();
    for part in names {
        let Ok(data) = read_part_bytes(zip, &part, MAX_XML_BYTES) else {
            continue;
        };
        if XmlEl::parse(data.as_slice()).is_err() {
            continue;
        }
        out.insert(
            part.clone(),
            ChartInfo {
                kind: super::chart::plot_kind(&data),
                workbook_part: first_rels_target(zip, &part, "package"),
            },
        );
    }
    out
}

/// Context threaded through a single slide's element extraction.
struct PageCtx<'a> {
    page_no: usize,
    slots: &'a SlotColors,
    defaults: MasterDefaults,
    master_line_spacing: Option<f64>,
    para_line_spacing: Option<f64>,
    para_margin_top: Option<f64>,
    master_spc_bef: Option<f64>,
    layout_placeholders: BTreeMap<PhKey, PlaceholderProto>,
    fonts: ThemeFonts,
    used_ids: BTreeSet<String>,
    media: &'a mut BTreeMap<String, String>,
    skipped: &'a mut Vec<Skipped>,
    losses: &'a mut LossTracker,
    charts: &'a HashMap<String, ChartInfo>,
    fallback_bg: Option<Fill>,
    layout_key: Option<String>,
    groups: HashMap<String, GroupDef>,
    group_seq: u32,
    in_master: bool,
}

impl<'a> PageCtx<'a> {
    fn skip(&mut self, name: &str, reason: impl Into<String>) {
        self.skipped.push(Skipped {
            page: self.page_no,
            name: name.to_string(),
            reason: reason.into(),
            loss: None,
        });
    }

    fn loss(&mut self, kind: LossKind, name: &str) {
        self.losses.record(kind, self.page_no, name);
    }

    fn skip_loss(&mut self, name: &str, reason: impl Into<String>, kind: LossKind) {
        self.losses.record(kind, self.page_no, name);
        self.skipped.push(Skipped {
            page: self.page_no,
            name: name.to_string(),
            reason: reason.into(),
            loss: Some(kind),
        });
    }

    fn next_group_id(&mut self) -> String {
        self.group_seq += 1;
        format!("grp{}", self.group_seq)
    }

    fn unique_id(&mut self, base: &str, fallback: &str) -> String {
        let base = if base.trim().is_empty() {
            fallback
        } else {
            base.trim()
        };
        if self.used_ids.insert(base.to_string()) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base} {n}");
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Placeholders & layouts
// ---------------------------------------------------------------------------

/// Placeholder match key: OOXML `type` (absent → "body") plus optional `idx`.
type PhKey = (String, Option<String>);

/// Inherited geometry + text defaults of a layout/master placeholder.
#[derive(Clone)]
struct PlaceholderProto {
    xfrm: Xform,
    prst: Option<String>,
    anchor: Option<VerticalAlign>,
    defaults: MasterDefaults,
}

fn ph_key(ph: &XmlEl) -> PhKey {
    (
        attr(ph, "type").unwrap_or("body").to_string(),
        attr(ph, "idx").map(str::to_string),
    )
}

/// Look up an inherited placeholder by `(type, idx)`, falling back to
/// `(type, None)` and finally to any entry with the same `type` so a slide
/// placeholder without `idx` can still match `(type, Some("0"))`.
fn lookup_placeholder<'a>(
    map: &'a BTreeMap<PhKey, PlaceholderProto>,
    key: &PhKey,
) -> Option<&'a PlaceholderProto> {
    map.get(key)
        .or_else(|| map.get(&(key.0.clone(), None)))
        .or_else(|| {
            map.iter()
                .find(|((ty, _), _)| ty == &key.0)
                .map(|(_, proto)| proto)
        })
}

/// Capture a layout/master placeholder's geometry + `lstStyle` defaults,
/// merging onto `fallback` when the layout omits `<a:xfrm>` or attributes.
fn placeholder_proto(
    el: &XmlEl,
    slots: &SlotColors,
    fallback: Option<&PlaceholderProto>,
) -> Option<PlaceholderProto> {
    let sp_pr = first(el, "spPr");
    let xfrm = sp_pr
        .and_then(|p| first(p, "xfrm"))
        .map(Xform::parse)
        .or_else(|| fallback.map(|f| f.xfrm))?;
    let prst = sp_pr
        .and_then(|p| first(p, "prstGeom"))
        .and_then(|g| attr(g, "prst"))
        .map(str::to_string)
        .or_else(|| fallback.and_then(|f| f.prst.clone()));

    let mut defaults = fallback.map(|f| f.defaults.clone()).unwrap_or_default();
    if let Some(lvl1) = first(el, "txBody")
        .and_then(|tb| first(tb, "lstStyle"))
        .and_then(|lst| first(lst, "lvl1pPr"))
    {
        if let Some(rpr) = first(lvl1, "defRPr") {
            let own = def_rpr_defaults(rpr, slots);
            if own.sz.is_some() {
                defaults.sz = own.sz;
            }
            if own.color.is_some() {
                defaults.color = own.color;
            }
            if own.latin_typeface.is_some() {
                defaults.latin_typeface = own.latin_typeface;
            }
            if own.bold.is_some() {
                defaults.bold = own.bold;
            }
            if own.italic.is_some() {
                defaults.italic = own.italic;
            }
        }
        if let Some(ln) = first(lvl1, "lnSpc")
            .and_then(|ls| first(ls, "spcPct"))
            .and_then(|sp| attr(sp, "val"))
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v / 100000.0)
        {
            defaults.line_spacing = Some(ln);
        }
        if let Some(sb) = lvl1_spc_bef(lvl1, defaults.sz) {
            defaults.spc_bef = Some(sb);
        }
    }
    let anchor = first(el, "txBody")
        .and_then(|tb| first(tb, "bodyPr"))
        .and_then(|bp| attr(bp, "anchor"))
        .map(|a| match a {
            "t" => VerticalAlign::Top,
            "ctr" => VerticalAlign::Middle,
            "b" => VerticalAlign::Bottom,
            _ => VerticalAlign::Top,
        })
        .or_else(|| fallback.and_then(|f| f.anchor));

    Some(PlaceholderProto {
        xfrm,
        prst,
        anchor,
        defaults,
    })
}

struct LayoutBuild {
    key: String,
    def: LayoutDef,
    protos: BTreeMap<PhKey, PlaceholderProto>,
    slots: SlotColors,
    defaults: MasterDefaults,
    fonts: ThemeFonts,
    fallback_bg: Option<Fill>,
}

fn layout_key(layout_part: &str) -> String {
    let stem = layout_part.rsplit('/').next().unwrap_or(layout_part);
    let stem = stem.trim_end_matches(".xml");
    let digits: String = stem
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if digits.is_empty() {
        format!("layout_{}", stem.replace('.', "_"))
    } else {
        format!("layout_{digits}")
    }
}

fn layout_display_name_from(layout_el: &XmlEl) -> Option<String> {
    let c_sld = first(layout_el, "cSld")?;
    let name = attr(c_sld, "name")?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

fn reserve_layout_key(key: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(key.clone()) {
        return key;
    }
    let base = key;
    for n in 2.. {
        let key = format!("{base}_{n}");
        if used.insert(key.clone()) {
            return key;
        }
    }
    unreachable!("layout key sequence terminates")
}

fn placeholder_def(proto: &PlaceholderProto, fonts: &ThemeFonts) -> PlaceholderDef {
    PlaceholderDef {
        bounds: to_bounds(&proto.xfrm),
        style: None,
        color: proto.defaults.color.clone(),
        font_size: proto.defaults.sz,
        font_family: proto
            .defaults
            .latin_typeface
            .as_deref()
            .and_then(|tf| resolve_typeface(tf, fonts))
            .map(FontFamily::Single),
        bold: proto.defaults.bold,
        italic: proto.defaults.italic,
        align: None,
    }
}

fn extract_notes_from_el(root: &XmlEl) -> Option<String> {
    let c_sld = first(root, "cSld")?;
    let sp_tree = first(c_sld, "spTree")?;
    for sp in children(sp_tree, "sp") {
        let Some(nv_sp_pr) = first(sp, "nvSpPr") else {
            continue;
        };
        let Some(nv_pr) = first(nv_sp_pr, "nvPr") else {
            continue;
        };
        let is_body = first(nv_pr, "ph")
            .and_then(|p| attr(p, "type"))
            .is_some_and(|t| t == "body");
        if !is_body {
            continue;
        }
        let Some(tx_body) = first(sp, "txBody") else {
            continue;
        };
        let mut lines = Vec::new();
        for p in children(tx_body, "p") {
            let mut line = String::new();
            for child in p.children.iter().filter_map(|n| n.as_element()) {
                match child.name.as_str() {
                    "r" | "fld" => {
                        if let Some(t) = first(child, "t").and_then(|el| el.get_text()) {
                            line.push_str(&t);
                        }
                    }
                    "br" => line.push('\n'),
                    _ => {}
                }
            }
            lines.push(line);
        }
        let text = lines.join("\n").trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

fn extract_notes(zip: &mut zip::ZipArchive<fs::File>, notes_part: &str) -> Option<String> {
    let root = parse_part(zip, notes_part).ok()?;
    extract_notes_from_el(&root)
}

#[allow(clippy::too_many_arguments)]
fn build_layout(
    zip: &mut zip::ZipArchive<fs::File>,
    layout_part: &str,
    master_part: Option<&str>,
    fallback_slots: &SlotColors,
    theme_cache: &mut HashMap<String, XmlEl>,
    master_cache: &mut HashMap<String, MasterBundle>,
    charts: &HashMap<String, ChartInfo>,
    media: &mut BTreeMap<String, String>,
    skipped: &mut Vec<Skipped>,
    losses: &mut LossTracker,
    used_layout_keys: &mut BTreeSet<String>,
) -> Result<LayoutBuild> {
    // Parse the layout XML once (used for display name, spTree, and background).
    let layout_el = parse_part(zip, layout_part).ok();
    let key = reserve_layout_key(
        layout_el
            .as_ref()
            .and_then(layout_display_name_from)
            .unwrap_or_else(|| layout_key(layout_part)),
        used_layout_keys,
    );
    let layout_rels_map = layout_rels(zip, layout_part);

    // Load the layout's master bundle once from cache (per-master clrMap + palette).
    if let Some(master) = master_part {
        let _ = load_master_bundle(zip, master, theme_cache, master_cache);
    }
    let master_bundle = master_part.and_then(|m| master_cache.get(m));
    let (layout_slots, defaults, fonts, fallback_bg) = match master_bundle {
        Some(bundle) => (
            if bundle.slots.is_empty() {
                fallback_slots.clone()
            } else {
                bundle.slots.clone()
            },
            bundle.defaults.clone(),
            bundle.fonts.clone(),
            bundle.bg.clone(),
        ),
        None => (
            fallback_slots.clone(),
            MasterDefaults::default(),
            ThemeFonts::default(),
            None,
        ),
    };

    let mut elements = Vec::new();
    let protos: BTreeMap<PhKey, PlaceholderProto>;
    let groups: HashMap<String, GroupDef>;
    {
        let mut lctx = PageCtx {
            page_no: 0,
            slots: &layout_slots,
            defaults: defaults.clone(),
            master_line_spacing: defaults.line_spacing,
            master_spc_bef: defaults.spc_bef,
            para_line_spacing: None,
            para_margin_top: None,
            layout_placeholders: BTreeMap::new(),
            fonts: fonts.clone(),
            used_ids: BTreeSet::new(),
            media,
            skipped,
            losses,
            charts,
            fallback_bg: fallback_bg.clone(),
            layout_key: None,
            groups: HashMap::new(),
            group_seq: 0,
            in_master: false,
        };
        if let Some(bundle) = master_bundle {
            if let Some(tree) = first(&bundle.xml, "cSld").and_then(|c| first(c, "spTree")) {
                lctx.in_master = true;
                for child in tree.children.iter().filter_map(|n| n.as_element()) {
                    let mut generated = Vec::new();
                    walk_sp_tree_child(
                        child,
                        &bundle.rels,
                        &mut lctx,
                        None,
                        None,
                        true,
                        &mut generated,
                    )?;
                    elements.extend(generated.into_iter().flatten());
                }
                lctx.in_master = false;
            }
        }
        if let Some(ref le) = layout_el {
            if let Some(tree) = first(le, "cSld").and_then(|c| first(c, "spTree")) {
                for child in tree.children.iter().filter_map(|n| n.as_element()) {
                    let mut generated = Vec::new();
                    walk_sp_tree_child(
                        child,
                        &layout_rels_map,
                        &mut lctx,
                        None,
                        None,
                        true,
                        &mut generated,
                    )?;
                    elements.extend(generated.into_iter().flatten());
                }
            }
        }
        protos = std::mem::take(&mut lctx.layout_placeholders);
        groups = std::mem::take(&mut lctx.groups);
    }

    let groups = if groups.is_empty() {
        None
    } else {
        Some(groups)
    };

    let background = layout_el
        .as_ref()
        .and_then(|l| bg_fill(l, &layout_slots))
        .or_else(|| fallback_bg.clone());

    // Deduplicate against placeholder shapes already emitted into `elements` so
    // rebuilding does not write two `<p:ph type="...">` shapes into slideLayout.
    let emitted_placeholders: BTreeSet<String> = elements
        .iter()
        .filter_map(|e| match e {
            Element::Text(t) => t.placeholder.clone(),
            _ => None,
        })
        .collect();
    let placeholders = protos
        .iter()
        .filter(|(k, _)| !emitted_placeholders.contains(&k.0))
        .map(|(k, p)| (k.0.clone(), placeholder_def(p, &fonts)))
        .collect();

    Ok(LayoutBuild {
        key,
        def: LayoutDef {
            background,
            elements,
            placeholders,
            groups,
        },
        protos,
        slots: layout_slots,
        defaults,
        fonts,
        fallback_bg,
    })
}

// ---------------------------------------------------------------------------
// Slide extraction & group walking
// ---------------------------------------------------------------------------

fn extract_slide(
    slide: &XmlEl,
    part: &str,
    ctx: &mut PageCtx<'_>,
    rels: &BTreeMap<String, String>,
    notes: Option<String>,
) -> Result<Page> {
    let c_sld = first(slide, "cSld")
        .ok_or_else(|| Error::Invalid(format!("slide part {part} has no p:cSld")))?;
    let sp_tree = first(c_sld, "spTree")
        .ok_or_else(|| Error::Invalid(format!("slide part {part} has no p:spTree")))?;

    let background = if ctx.layout_key.is_some() {
        bg_fill(slide, ctx.slots)
    } else {
        bg_fill(slide, ctx.slots).or_else(|| ctx.fallback_bg.clone())
    };

    let mut elements = Vec::new();
    for child in sp_tree.children.iter().filter_map(|n| n.as_element()) {
        let mut generated = Vec::new();
        walk_sp_tree_child(child, rels, ctx, None, None, false, &mut generated)?;
        elements.extend(generated.into_iter().flatten());
    }

    Ok(Page {
        version: None,
        title: None,
        page_type: None,
        layout: ctx.layout_key.clone(),
        background,
        notes,
        elements,
        animations: None,
        groups: if ctx.groups.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut ctx.groups))
        },
    })
}

fn child_local_xfrm(child: &XmlEl) -> Option<&XmlEl> {
    first(child, "spPr")
        .and_then(|sp_pr| first(sp_pr, "xfrm"))
        .or_else(|| first(child, "xfrm"))
}

fn walk_sp_tree_child(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    group: Option<&Xform>,
    group_id: Option<&str>,
    in_layout: bool,
    out: &mut Vec<Option<Element>>,
) -> Result<()> {
    match el.name.as_str() {
        "sp" => out.push(map_sp(el, rels, ctx, group, in_layout)?),
        "pic" => out.push(map_pic(el, rels, ctx, group)),
        "cxnSp" => out.push(map_line(el, rels, ctx, group)),
        "grpSp" => {
            flatten_group(el, rels, ctx, group, group_id, None, in_layout, out)?;
        }
        "graphicFrame" => {
            let name = cnv_name(el).unwrap_or_else(|| "graphicFrame".into());
            let uri = graphic_uri(el).unwrap_or_default();
            if uri.contains("/table") {
                out.push(map_table(el, rels, ctx, group));
            } else if uri.contains("/chart") {
                out.push(map_chart(el, rels, ctx, group));
            } else {
                ctx.skip(
                    &name,
                    "graphicFrame elements are not representable in PPTD",
                );
                out.push(None);
            }
        }
        "oleObj" => {
            ctx.skip(
                cnv_name(el).unwrap_or_else(|| "oleObj".into()).as_str(),
                "embedded objects are not supported",
            );
            out.push(None);
        }
        _ => {
            out.push(None);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn flatten_group(
    el: &XmlEl,
    rels: &BTreeMap<String, String>,
    ctx: &mut PageCtx<'_>,
    outer: Option<&Xform>,
    parent_id: Option<&str>,
    outer_fill: Option<&Fill>,
    in_layout: bool,
    out: &mut Vec<Option<Element>>,
) -> Result<()> {
    let group_fill: Option<Fill> = first(el, "grpSpPr").and_then(|gpr| {
        if first(gpr, "noFill").is_some() {
            return None;
        }
        if first(gpr, "grpFill").is_some() {
            return outer_fill.cloned();
        }
        if let Some(solid) = first(gpr, "solidFill") {
            return color_from_fill(solid, ctx.slots).map(|color| Fill::Solid { color });
        }
        if let Some(grad) = first(gpr, "gradFill") {
            return gradient_from(grad, ctx.slots);
        }
        None
    });
    let Some(grel) = first(el, "grpSpPr").and_then(|p| first(p, "xfrm")) else {
        for child in el.children.iter().filter_map(|n| n.as_element()) {
            match child.name.as_str() {
                "nvGrpSpPr" | "grpSpPr" => continue,
                "grpSp" => {
                    flatten_group(
                        child,
                        rels,
                        ctx,
                        outer,
                        parent_id,
                        group_fill.as_ref(),
                        in_layout,
                        out,
                    )?;
                }
                _ => {
                    let before = out.len();
                    walk_sp_tree_child(child, rels, ctx, outer, parent_id, in_layout, out)?;
                    for elem in out[before..].iter_mut().flatten() {
                        if let Some(parent) = parent_id {
                            elem.common_mut().group_id = Some(parent.to_owned());
                            if let Some(xel) = child_local_xfrm(child) {
                                elem.common_mut().group_bounds =
                                    Some(to_bounds(&Xform::parse(xel)));
                            }
                        }
                        if let Some(sp_pr) = first(child, "spPr") {
                            if first(sp_pr, "grpFill").is_some() {
                                if let Element::Shape(shape) = elem {
                                    if shape.fill.is_none() {
                                        shape.fill = group_fill.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(());
    };
    let raw = Xform::parse(grel);
    let g = raw.apply(outer);
    let ext_sub = raw.ext.0.abs() < 12700.0 && raw.ext.1.abs() < 12700.0;
    let ch_sub = raw.ch_ext.0.abs() < 12700.0 && raw.ch_ext.1.abs() < 12700.0;
    let enlarging_stub = ch_sub && !ext_sub;
    if enlarging_stub {
        let group_id = ctx.next_group_id();
        ctx.groups.insert(
            group_id.clone(),
            GroupDef {
                xfrm: GroupXfrm {
                    off: (px(raw.off.0), px(raw.off.1)),
                    ext: (px(raw.ext.0), px(raw.ext.1)),
                    ch_off: (px(raw.ch_off.0), px(raw.ch_off.1)),
                    ch_ext: (px(raw.ch_ext.0), px(raw.ch_ext.1)),
                    rot: raw.rot,
                    flip: raw.flip.filter(|(h, v)| *h || *v),
                },
                name: cnv_name(el),
                fill: group_fill.clone(),
                parent: parent_id.map(str::to_owned),
            },
        );
        for child in el.children.iter().filter_map(|n| n.as_element()) {
            match child.name.as_str() {
                "nvGrpSpPr" | "grpSpPr" => continue,
                "grpSp" => {
                    flatten_group(
                        child,
                        rels,
                        ctx,
                        Some(&g),
                        Some(&group_id),
                        group_fill.as_ref(),
                        in_layout,
                        out,
                    )?;
                }
                _ => {
                    let before = out.len();
                    walk_sp_tree_child(
                        child,
                        rels,
                        ctx,
                        Some(&g),
                        Some(&group_id),
                        in_layout,
                        out,
                    )?;
                    for elem in out[before..].iter_mut().flatten() {
                        elem.common_mut().group_id = Some(group_id.clone());
                        if let Some(xel) = child_local_xfrm(child) {
                            elem.common_mut().group_bounds = Some(to_bounds(&Xform::parse(xel)));
                        }
                        if let Some(sp_pr) = first(child, "spPr") {
                            if first(sp_pr, "grpFill").is_some() {
                                if let Element::Shape(shape) = elem {
                                    if shape.fill.is_none() {
                                        shape.fill = group_fill.clone();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        return Ok(());
    }
    for child in el.children.iter().filter_map(|n| n.as_element()) {
        match child.name.as_str() {
            "nvGrpSpPr" | "grpSpPr" => continue,
            "grpSp" => {
                flatten_group(
                    child,
                    rels,
                    ctx,
                    Some(&g),
                    parent_id,
                    group_fill.as_ref(),
                    in_layout,
                    out,
                )?;
            }
            _ => {
                let before = out.len();
                walk_sp_tree_child(child, rels, ctx, Some(&g), None, in_layout, out)?;
                for elem in out[before..].iter_mut().flatten() {
                    if let Some(sp_pr) = first(child, "spPr") {
                        if first(sp_pr, "grpFill").is_some() {
                            if let Element::Shape(shape) = elem {
                                if shape.fill.is_none() {
                                    shape.fill = group_fill.clone();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::opc::{
        external_target, extract_sld_id_rids, register_media, resolve_target, xml_bool,
    };
    use super::shapes::GeomGuides;
    use super::text::{RunStyle, text_content};
    use super::theme::{col_with_modifiers, preset_color_hex};
    use super::*;

    fn parse(xml: &str) -> XmlEl {
        parse_xml(xml.as_bytes()).expect("valid test XML")
    }

    fn page_ctx<'a>(
        slots: &'a SlotColors,
        media: &'a mut BTreeMap<String, String>,
        skipped: &'a mut Vec<Skipped>,
        losses: &'a mut LossTracker,
        charts: &'a HashMap<String, ChartInfo>,
    ) -> PageCtx<'a> {
        PageCtx {
            page_no: 0,
            slots,
            defaults: MasterDefaults::default(),
            master_line_spacing: None,
            para_line_spacing: None,
            para_margin_top: None,
            master_spc_bef: None,
            layout_placeholders: BTreeMap::new(),
            fonts: fonts(),
            used_ids: BTreeSet::new(),
            media,
            skipped,
            losses,
            charts,
            fallback_bg: None,
            layout_key: None,
            groups: HashMap::new(),
            group_seq: 0,
            in_master: false,
        }
    }

    #[test]
    fn whitespace_only_runs_survive_extraction() {
        let tx_body = parse(
            r#"<txBody xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<a:p><a:r><a:rPr b="1"/><a:t>Intro</a:t></a:r>
<a:r><a:rPr><a:solidFill><a:srgbClr val="2A2724"/></a:solidFill></a:rPr><a:t xml:space="preserve"> </a:t></a:r>
<a:r><a:rPr><a:solidFill><a:srgbClr val="58758A"/></a:solidFill><a:hlinkClick r:id="rId1"/></a:rPr><a:t>arXiv:2605.27223</a:t></a:r></a:p>
</txBody>"#,
        );
        let slots = SlotColors::new();
        let mut media = BTreeMap::new();
        let mut skipped = Vec::new();
        let mut losses = LossTracker::default();
        let charts = HashMap::new();
        let rels = BTreeMap::from([(
            "rId1".to_string(),
            "https://arxiv.org/abs/2605.27223".to_string(),
        )]);
        let mut ctx = page_ctx(&slots, &mut media, &mut skipped, &mut losses, &charts);
        let content =
            text_content(&tx_body, &rels, &slots, &mut ctx, "t", true).expect("box has text");
        assert!(
            content.text.contains("Intro<span") && content.text.contains("> </span><a href"),
            "spacer run lost: {}",
            content.text
        );

        let tx_body = parse(
            r#"<txBody xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:p><a:r><a:t xml:space="preserve">  padded label  </a:t></a:r></a:p></txBody>"#,
        );
        let mut ctx = page_ctx(&slots, &mut media, &mut skipped, &mut losses, &charts);
        let content = text_content(&tx_body, &BTreeMap::new(), &slots, &mut ctx, "t", true)
            .expect("box has text");
        assert!(
            content.text.contains("  padded label  "),
            "edge spaces trimmed: {}",
            content.text
        );
    }

    #[test]
    fn whitespace_runs_at_paragraph_edges_survive_extraction() {
        let tx_body = parse(
            r#"<txBody xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:p><a:r><a:t xml:space="preserve"> </a:t></a:r><a:r><a:rPr b="1"/><a:t>x</a:t></a:r><a:r><a:t xml:space="preserve"> </a:t></a:r></a:p></txBody>"#,
        );
        let slots = SlotColors::new();
        let mut media = BTreeMap::new();
        let mut skipped = Vec::new();
        let mut losses = LossTracker::default();
        let charts = HashMap::new();
        let mut ctx = page_ctx(&slots, &mut media, &mut skipped, &mut losses, &charts);
        let content = text_content(&tx_body, &BTreeMap::new(), &slots, &mut ctx, "t", true)
            .expect("box has text");
        let inner = content
            .text
            .trim_start_matches("<p>")
            .trim_end_matches("</p>");
        assert!(
            inner.starts_with(' ') && inner.ends_with(' '),
            "edge spacer run lost: {}",
            content.text
        );
    }

    fn fonts() -> ThemeFonts {
        ThemeFonts {
            major: Some("Calibri Light".into()),
            minor: Some("Calibri".into()),
        }
    }

    fn defaults() -> MasterDefaults {
        MasterDefaults {
            sz: Some(2835.0 / 100.0),
            color: Some(Color("#000000".into())),
            latin_typeface: Some("+mn-lt".into()),
            bold: None,
            italic: None,
            line_spacing: None,
            title_line_spacing: None,
            spc_bef: None,
            title_spc_bef: None,
        }
    }

    #[test]
    fn run_without_explicit_attrs_inherits_master_defaults() {
        let rpr = parse(r#"<rPr lang="en-US"/>"#);
        let st = RunStyle::from_rpr(Some(&rpr), &SlotColors::new(), &defaults(), &fonts())
            .expect("rPr must yield a style");
        assert_eq!(st.font_size, Some(28.35));
        assert_eq!(st.color.as_ref().map(|c| c.0.as_str()), Some("#000000"));
        assert_eq!(
            st.font_family,
            Some(FontFamily::Single("Calibri".to_string())),
            "+mn-lt alias must resolve to the theme minor font"
        );
    }

    #[test]
    fn explicit_attrs_win_over_defaults() {
        let rpr = parse(
            r#"<rPr sz="4000" b="1"><solidFill><srgbClr val="0D37D4"/></solidFill><latin typeface="微软雅黑"/></rPr>"#,
        );
        let st = RunStyle::from_rpr(Some(&rpr), &SlotColors::new(), &defaults(), &fonts())
            .expect("rPr must yield a style");
        assert_eq!(st.font_size, Some(40.0));
        assert!(st.bold == Some(true));
        assert_eq!(st.color.as_ref().map(|c| c.0.as_str()), Some("#0D37D4"));
        assert_eq!(
            st.font_family,
            Some(FontFamily::Single("微软雅黑".to_string()))
        );
        assert_eq!(st.italic, None);
    }

    #[test]
    fn master_default_color_needs_theme_slots() {
        let rpr = parse(r#"<rPr><solidFill><schemeClr val="tx1"/></solidFill></rPr>"#);
        let mut slots = SlotColors::new();
        slots.insert("dk1".into(), "000000".into());
        slots.insert("tx1".into(), "000000".into());
        let st = RunStyle::from_rpr(Some(&rpr), &slots, &MasterDefaults::default(), &fonts())
            .expect("rPr must yield a style");
        assert_eq!(st.color.as_ref().map(|c| c.0.as_str()), Some("#000000"));
    }

    #[test]
    fn master_body_title_line_spacing_is_captured() {
        let master_xml = r#"<?xml version="1.0"?><p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:txStyles>
  <p:titleStyle>
    <a:lvl1pPr><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="50000"/></a:spcBef><a:defRPr sz="4400"/></a:lvl1pPr>
  </p:titleStyle>
  <p:bodyStyle>
    <a:lvl1pPr><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="50000"/></a:spcBef><a:defRPr sz="2800"/></a:lvl1pPr>
    <a:lvl2pPr><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:defRPr sz="2400"/></a:lvl2pPr>
  </p:bodyStyle>
  <p:otherStyle>
    <a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr>
  </p:otherStyle>
</p:txStyles>
</p:sldMaster>"#;
        let master_el = parse(master_xml);
        let d = master_defaults(&master_el, &SlotColors::new());
        assert_eq!(d.sz, Some(18.0), "otherStyle defRPr sz=1800 → 18pt");
        assert_eq!(d.line_spacing, Some(0.9));
        assert_eq!(d.title_line_spacing, Some(0.9));
        // Each style scales spcPct by its own defRPr@sz (28*0.5=14, 44*0.5=22), not otherStyle's 18.
        assert_eq!(d.spc_bef, Some(14.0));
        assert_eq!(d.title_spc_bef, Some(22.0));
    }

    #[test]
    fn placeholder_proto_captures_body_pr_anchor() {
        let el = parse(
            r#"<?xml version="1.0"?><p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:nvSpPr><p:cNvPr name="Title Placeholder"/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="1000" y="1000"/><a:ext cx="5000" cy="2000"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr>
<p:txBody><a:bodyPr anchor="ctr" lIns="91440" tIns="45720" rIns="91440" bIns="45720"/><a:p><a:r><a:t>x</a:t></a:r></a:p></p:txBody></p:sp>"#,
        );
        let proto = placeholder_proto(&el, &SlotColors::new(), None).expect("placeholder proto");
        assert_eq!(proto.anchor, Some(VerticalAlign::Middle));
        let el2 = parse(
            r#"<?xml version="1.0"?><p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:nvSpPr><p:cNvPr name="Body Placeholder"/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1000" cy="1000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/></p:txBody></p:sp>"#,
        );
        let proto2 = placeholder_proto(&el2, &SlotColors::new(), None).expect("placeholder proto");
        assert_eq!(proto2.anchor, None);
    }

    #[test]
    fn opc_root_targets_sld_id_rids_and_external_links() {
        assert_eq!(
            resolve_target("ppt/presentation.xml", "/ppt/slides/slide1.xml"),
            "ppt/slides/slide1.xml"
        );
        assert_eq!(
            resolve_target("ppt/slides/slide1.xml", "../media/image1.png"),
            "ppt/media/image1.png"
        );
        let rids = extract_sld_id_rids(
            r#"<p:sldIdLst><p:sldId r:id="rId3" id="256"/><p:sldId id="257" r:id="rId4"/></p:sldIdLst>"#,
        );
        assert_eq!(rids, vec![Some("rId3".into()), Some("rId4".into())]);

        let rels = BTreeMap::from([
            ("r1".into(), "https://example.com".into()),
            ("r2".into(), "mailto:a@b.com".into()),
            ("r3".into(), "ppaction://hlinkshowjump?jump=nextslide".into()),
            ("r4".into(), "ppt/slides/slide2.xml".into()),
        ]);
        assert_eq!(external_target(&rels, "r1").as_deref(), Some("https://example.com"));
        assert_eq!(external_target(&rels, "r2").as_deref(), Some("mailto:a@b.com"));
        assert_eq!(external_target(&rels, "r3"), None);
        assert_eq!(external_target(&rels, "r4"), None);
    }

    #[test]
    fn media_registration_sanitizes_and_deduplicates_filenames() {
        let mut media = BTreeMap::new();
        let a = register_media(&mut media, r"ppt/media/..\..\evil.png".into());
        assert!(!a.contains("..") && !a.contains('\\') && !a.contains('/'), "got {a}");
        let b = register_media(&mut media, "ppt/media/a/pic.png".into());
        let c = register_media(&mut media, "ppt/media/b/pic.png".into());
        assert_ne!(b, c, "distinct archive parts sharing a basename must not collide");
    }

    #[test]
    fn color_modifiers_tint_hsl_dark_clrmap_and_non_ascii_safety() {
        // tint="100000" (100% original color) keeps red as #FF0000; tint="50000" blends halfway to white.
        let t100 = parse(r#"<srgbClr val="FF0000"><tint val="100000"/></srgbClr>"#);
        assert_eq!(col_with_modifiers("FF0000", None, &t100).0, "#FF0000");
        let t50 = parse(r#"<srgbClr val="FF0000"><tint val="50000"/></srgbClr>"#);
        assert_eq!(col_with_modifiers("FF0000", None, &t50).0, "#FF8080");
        // satMod on pure white must stay white (not turn #808080).
        let sat = parse(r#"<srgbClr val="FFFFFF"><satMod val="200000"/></srgbClr>"#);
        assert_eq!(col_with_modifiers("FFFFFF", None, &sat).0, "#FFFFFF");
        // Non-ASCII 6-byte string must not panic.
        let bad = parse(r#"<srgbClr val="蓝色"/>"#);
        let _ = col_with_modifiers("蓝色", None, &bad);
        // Preset green is CSS/OOXML #008000 (not lime #00FF00), and ltBlue is present.
        assert_eq!(preset_color_hex("green"), "008000");
        assert_eq!(preset_color_hex("ltBlue"), "ADD8E6");

        // Dark-mode clrMap flips bg1->dk1 and tx1->lt1.
        let theme = parse(
            r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Dark"><a:dk1><a:srgbClr val="111111"/></a:dk1><a:lt1><a:srgbClr val="EEEEEE"/></a:lt1><a:dk2><a:srgbClr val="222222"/></a:dk2><a:lt2><a:srgbClr val="DDDDDD"/></a:lt2></a:clrScheme></a:themeElements></a:theme>"#,
        );
        let master = parse(
            r#"<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:clrMap bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2"/></p:sldMaster>"#,
        );
        let slots = read_theme_slots_from(&theme, Some(&master));
        assert_eq!(slots.get("bg1").map(String::as_str), Some("111111"));
        assert_eq!(slots.get("tx1").map(String::as_str), Some("EEEEEE"));
    }

    #[test]
    fn xml_bool_geom_guides_and_extract_notes_edge_cases() {
        let el = parse(r#"<rPr b="true" i="false" u="1" strike="0"/>"#);
        assert_eq!(xml_bool(&el, "b"), Some(true));
        assert_eq!(xml_bool(&el, "i"), Some(false));
        assert_eq!(xml_bool(&el, "u"), Some(true));
        assert_eq!(xml_bool(&el, "strike"), Some(false));

        // GeomGuides: sin(1000, 90deg=5400000) == 1000, cos(1000, 0) == 1000, cat2(1000, 1, 0) == 1000.
        let mut g = GeomGuides::new((0.0, 0.0, 1000.0, 1000.0), &[]);
        g.evaluate("s", "sin 1000 5400000").unwrap();
        g.evaluate("c", "cos 1000 0").unwrap();
        g.evaluate("k", "cat2 1000 1 0").unwrap();
        assert!((g.get("s").unwrap() - 1000.0).abs() < 1e-3);
        assert!((g.get("c").unwrap() - 1000.0).abs() < 1e-3);
        assert!((g.get("k").unwrap() - 1000.0).abs() < 1e-3);

        // extract_notes skips an earlier shape missing txBody/nvPr and preserves <a:br/>.
        let notes_el = parse(
            r#"<p:notes xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr/><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Line 1</a:t></a:r><a:br/><a:r><a:t>Line 2</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#,
        );
        assert_eq!(extract_notes_from_el(&notes_el).as_deref(), Some("Line 1\nLine 2"));
    }
}
