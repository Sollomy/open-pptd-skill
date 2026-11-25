//! OPC package navigation, bounded ZIP reading, XML helpers, and media naming.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Read;

use xmltree::Element as XmlEl;

use crate::{Error, Result};

use super::EMU_PER_PX;

/// Maximum uncompressed size for any XML part read from an OPC archive (32 MiB).
pub(super) const MAX_XML_BYTES: u64 = 32 * 1024 * 1024;

/// Maximum uncompressed size for any media/chart/workbook asset copied out (64 MiB).
pub(super) const MAX_MEDIA_BYTES: u64 = 64 * 1024 * 1024;

/// Read a ZIP entry up to `max_bytes`, failing with [`Error::Invalid`] if the
/// decompressed stream exceeds the cap (guards against ZIP bombs).
pub(super) fn read_zip_limited(
    reader: impl Read,
    max_bytes: u64,
    label: &str,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    reader
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|e| Error::Invalid(format!("read {label}: {e}")))?;
    if data.len() as u64 > max_bytes {
        return Err(Error::Invalid(format!(
            "{label} exceeds maximum allowed size ({max_bytes} bytes)"
        )));
    }
    Ok(data)
}

/// Read raw bytes of an archive part with a decompressed size limit.
pub(super) fn read_part_bytes(
    zip: &mut zip::ZipArchive<fs::File>,
    part: &str,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    let reader = zip
        .by_name(part)
        .map_err(|e| Error::Invalid(format!("part {part} missing in package: {e}")))?;
    read_zip_limited(reader, max_bytes, &format!("part {part}"))
}

/// Parse an XML part from the archive (capped at [`MAX_XML_BYTES`]).
pub(super) fn parse_part(zip: &mut zip::ZipArchive<fs::File>, part: &str) -> Result<XmlEl> {
    let data = read_part_bytes(zip, part, MAX_XML_BYTES)?;
    parse_xml(data.as_slice()).map_err(|e| Error::Invalid(format!("parse XML in {part}: {e}")))
}

/// Parse an OOXML part without losing whitespace-only text.
///
/// `xml-rs` reports whitespace-only character data as `Whitespace` events and
/// `xmltree` drops those. DrawingML stores significant spaces that way —
/// `<a:t> </a:t>` (usually with `xml:space="preserve"`) is the canonical
/// spacer run between differently styled runs. Dropping the node silently
/// glued `Intro`/`arXiv:…` together.
pub(super) fn parse_xml(data: &[u8]) -> std::result::Result<XmlEl, xmltree::ParseError> {
    let config = xmltree::ParserConfig::new().whitespace_to_characters(true);
    let mut root = XmlEl::parse_with_config(data, config)?;
    drop_indentation(&mut root);
    Ok(root)
}

/// `whitespace_to_characters` also surfaces the indentation between elements;
/// keep text nodes only where the schema carries character data (`a:t`,
/// `m:t`). Everything else is formatting and must stay invisible to
/// `get_text`/`first` helpers.
fn drop_indentation(el: &mut XmlEl) {
    let text_bearing = el.name == "t";
    el.children.retain(|node| match node {
        xmltree::XMLNode::Text(text) => text_bearing || !text.trim().is_empty(),
        _ => true,
    });
    for child in el.children.iter_mut().filter_map(|n| n.as_mut_element()) {
        drop_indentation(child);
    }
}

pub(super) fn children<'a>(el: &'a XmlEl, name: &str) -> Vec<&'a XmlEl> {
    el.children
        .iter()
        .filter_map(|n| n.as_element())
        .filter(|c| c.name == name)
        .collect()
}

pub(super) fn first<'a>(el: &'a XmlEl, name: &str) -> Option<&'a XmlEl> {
    el.children
        .iter()
        .filter_map(|n| n.as_element())
        .find(|c| c.name == name)
}

/// First element with a matching local name anywhere in the subtree
/// (breadth-first, direct children first).
pub(super) fn first_descendant<'a>(el: &'a XmlEl, name: &str) -> Option<&'a XmlEl> {
    let mut queue: VecDeque<&XmlEl> = el.children.iter().filter_map(|n| n.as_element()).collect();
    while let Some(node) = queue.pop_front() {
        if node.name == name {
            return Some(node);
        }
        queue.extend(node.children.iter().filter_map(|n| n.as_element()));
    }
    None
}

pub(super) fn attr<'a>(el: &'a XmlEl, name: &str) -> Option<&'a str> {
    el.attributes.get(name).map(String::as_str)
}

/// Parse an OOXML `xsd:boolean` attribute (`"1"`/`"true"` → `true`,
/// `"0"`/`"false"` → `false`).
pub(super) fn xml_bool(el: &XmlEl, name: &str) -> Option<bool> {
    match attr(el, name)? {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Package navigation
// ---------------------------------------------------------------------------

/// Find the `ppt/presentation.xml` part through the package root rels.
pub(super) fn root_presentation_part(zip: &mut zip::ZipArchive<fs::File>) -> Result<String> {
    let root = parse_part(zip, "_rels/.rels")?;
    for rel in children(&root, "Relationship") {
        let ty = attr(rel, "Type").unwrap_or_default();
        let target = attr(rel, "Target").unwrap_or_default();
        if ty.ends_with("/officeDocument") {
            return Ok(target.trim_start_matches('/').to_string());
        }
    }
    Err(Error::Invalid(
        "package has no officeDocument relationship in _rels/.rels".into(),
    ))
}

/// Resolve a relationship target against the part's directory (or archive root
/// when `target` begins with `'/'`), yielding an archive-internal path like
/// `ppt/slides/slide1.xml`.
pub(super) fn resolve_target(part: &str, target: &str) -> String {
    let joined = if let Some(abs) = target.strip_prefix('/') {
        abs.to_string()
    } else {
        let dir = match part.rsplit_once('/') {
            Some((dir, _)) => dir,
            None => "",
        };
        format!("{dir}/{target}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Relationship part path for a main part: `ppt/slides/slide1.xml` →
/// `ppt/slides/_rels/slide1.xml.rels`.
pub(super) fn rels_part(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/_rels/{name}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// First relationship of the given type from the part's rels.
pub(super) fn first_rels_target(
    zip: &mut zip::ZipArchive<fs::File>,
    part: &str,
    type_suffix: &str,
) -> Option<String> {
    let rels_part = rels_part(part);
    let root = parse_part(zip, &rels_part).ok()?;
    for rel in children(&root, "Relationship") {
        let ty = attr(rel, "Type").unwrap_or_default();
        let target = attr(rel, "Target").unwrap_or_default();
        if ty.ends_with(type_suffix) {
            return Some(resolve_target(part, target));
        }
    }
    None
}

/// All relationship targets of `part` whose type ends with `type_suffix`,
/// in rels-file order (e.g. every slideLayout of a slideMaster).
pub(super) fn rels_targets(
    zip: &mut zip::ZipArchive<fs::File>,
    part: &str,
    type_suffix: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rels_root) = parse_part(zip, &rels_part(part)) {
        for rel in children(&rels_root, "Relationship") {
            let ty = attr(rel, "Type").unwrap_or_default();
            let target = attr(rel, "Target").unwrap_or_default();
            if ty.ends_with(type_suffix) {
                out.push(resolve_target(part, target));
            }
        }
    }
    out
}

/// Extract the relationship-id (`r:id` / `<prefix>:id`) attribute from every
/// `<p:sldId>` / `<sldId>` element in `p:sldIdLst` in document order.
///
/// `xmltree 0.11` keys attributes by local name only, so when `r:id` precedes
/// `id` (`<p:sldId r:id="rId3" id="256"/>`), the numeric `id` overwrites `r:id`.
/// Scanning the raw `presentation.xml` preserves the real relationship id
/// without introducing another XML parser dependency.
pub(super) fn extract_sld_id_rids(xml: &str) -> Vec<Option<String>> {
    let bytes = xml.as_bytes();
    let mut i = 0;
    let mut in_list = false;
    let mut rids = Vec::new();
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if xml[i..].starts_with("<!--") {
            if let Some(end) = xml[i + 4..].find("-->") {
                i += 4 + end + 3;
                continue;
            }
            break;
        }
        if xml[i..].starts_with("<![CDATA[") {
            if let Some(end) = xml[i + 9..].find("]]>") {
                i += 9 + end + 3;
                continue;
            }
            break;
        }
        if xml[i..].starts_with("<?") {
            if let Some(end) = xml[i + 2..].find("?>") {
                i += 2 + end + 2;
                continue;
            }
            break;
        }
        let tag_start = i + 1;
        let mut j = tag_start;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let b = bytes[j];
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
            } else if b == b'"' || b == b'\'' {
                quote = Some(b);
            } else if b == b'>' {
                break;
            }
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let tag_body = xml[tag_start..j].trim();
        i = j + 1;
        if tag_body.is_empty() || tag_body.starts_with('!') {
            continue;
        }
        let closing = tag_body.starts_with('/');
        let self_closing = !closing && tag_body.ends_with('/');
        let body = tag_body.trim_start_matches('/').trim_end_matches('/').trim();
        let name_end = body
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(body.len());
        let qname = &body[..name_end];
        let local = qname.rsplit_once(':').map(|(_, l)| l).unwrap_or(qname);
        if local == "sldIdLst" {
            if closing {
                if in_list {
                    break;
                }
            } else if !self_closing {
                in_list = true;
            }
            continue;
        }
        if in_list && !closing && local == "sldId" {
            let attrs_str = body[name_end..].trim();
            rids.push(extract_rel_id_attr(attrs_str));
        }
    }
    rids
}

fn extract_rel_id_attr(attrs: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name = &attrs[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let q = bytes[i];
        if q != b'"' && q != b'\'' {
            continue;
        }
        i += 1;
        let val_start = i;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let val = &attrs[val_start..i];
        i += 1;
        if name == "r:id" || name.ends_with(":id") {
            return Some(val.to_string());
        }
    }
    None
}

/// Slide parts in the order of `p:sldIdLst`.
pub(super) fn presentation_slides(
    pres: &XmlEl,
    pres_raw: &str,
    zip: &mut zip::ZipArchive<fs::File>,
    presentation_part: &str,
) -> Result<Vec<String>> {
    let sld_id_lst = first(pres, "sldIdLst")
        .ok_or_else(|| Error::Invalid("presentation.xml has no p:sldIdLst".into()))?;
    let rels_part = rels_part(presentation_part);
    let rels = parse_part(zip, &rels_part)?;
    let mut by_id = BTreeMap::new();
    for rel in children(&rels, "Relationship") {
        let id = attr(rel, "Id").unwrap_or_default().to_string();
        let target = attr(rel, "Target").unwrap_or_default();
        by_id.insert(id, resolve_target(presentation_part, target));
    }
    let raw_rids = extract_sld_id_rids(pres_raw);
    let mut out = Vec::new();
    for (idx, sld_id) in children(sld_id_lst, "sldId").into_iter().enumerate() {
        let rid = raw_rids
            .get(idx)
            .and_then(|r| r.as_deref())
            .or_else(|| attr(sld_id, "id"))
            .ok_or_else(|| Error::Invalid("p:sldId missing r:id".into()))?;
        let target = by_id
            .get(rid)
            .ok_or_else(|| Error::Invalid(format!("slide rId {rid} not in presentation rels")))?;
        out.push(target.clone());
    }
    Ok(out)
}

/// `p:sldSz` → design size in px (exact inverse of the writer).
pub(super) fn parse_size(pres: &XmlEl) -> Result<crate::pptd::shared::Size> {
    let sz = first(pres, "sldSz")
        .ok_or_else(|| Error::Invalid("presentation.xml has no p:sldSz".into()))?;
    let cx: i64 = attr(sz, "cx")
        .ok_or_else(|| Error::Invalid("sldSz missing cx".into()))?
        .parse()
        .map_err(|_| Error::Invalid("sldSz cx not an integer".into()))?;
    let cy: i64 = attr(sz, "cy")
        .ok_or_else(|| Error::Invalid("sldSz missing cy".into()))?
        .parse()
        .map_err(|_| Error::Invalid("sldSz cy not an integer".into()))?;
    Ok(crate::pptd::shared::Size {
        width: cx as f64 / EMU_PER_PX,
        height: cy as f64 / EMU_PER_PX,
    })
}

/// Parse a slide's `.rels` → (relationship map, layout part path, notes part path).
pub(super) fn parse_slide_rels(
    zip: &mut zip::ZipArchive<fs::File>,
    part: &str,
) -> (BTreeMap<String, String>, Option<String>, Option<String>) {
    let mut rels = BTreeMap::new();
    let mut layout_part = None;
    let mut notes_part = None;
    if let Ok(rels_root) = parse_part(zip, &rels_part(part)) {
        for rel in children(&rels_root, "Relationship") {
            let id = attr(rel, "Id").unwrap_or_default().to_string();
            let target = attr(rel, "Target").unwrap_or_default();
            let resolved = if attr(rel, "TargetMode") == Some("External") {
                target.to_string()
            } else {
                resolve_target(part, target)
            };
            if attr(rel, "Type").is_some_and(|t| t.ends_with("/slideLayout"))
                && layout_part.is_none()
            {
                layout_part = Some(resolved.clone());
            } else if attr(rel, "Type").is_some_and(|t| t.ends_with("/notesSlide"))
                && notes_part.is_none()
            {
                notes_part = Some(resolved.clone());
            }
            rels.insert(id, resolved);
        }
    }
    (rels, layout_part, notes_part)
}

/// Slide-layout / slide-master rels (targets resolved against the part).
pub(super) fn layout_rels(
    zip: &mut zip::ZipArchive<fs::File>,
    layout_part: &str,
) -> BTreeMap<String, String> {
    let mut rels = BTreeMap::new();
    if let Ok(rels_root) = parse_part(zip, &rels_part(layout_part)) {
        for rel in children(&rels_root, "Relationship") {
            let id = attr(rel, "Id").unwrap_or_default().to_string();
            let target = attr(rel, "Target").unwrap_or_default();
            let resolved = if attr(rel, "TargetMode") == Some("External") {
                target.to_string()
            } else {
                resolve_target(layout_part, target)
            };
            rels.insert(id, resolved);
        }
    }
    rels
}

/// Resolve a relationship id only when it points to an allowlisted external URL
/// scheme (`http://`, `https://`, `mailto:`). Internal jump actions
/// (`ppaction://…`, slide targets) and unsafe schemes (`file://`, `javascript:`)
/// are not represented in PPTD.
pub(super) fn external_target(rels: &BTreeMap<String, String>, rid: &str) -> Option<String> {
    let target = rels.get(rid)?;
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
    {
        Some(target.clone())
    } else {
        None
    }
}

/// Register an archive part into `media` (`part_path -> safe_basename`),
/// sanitizing unsafe path characters / Windows separators and deduplicating
/// colliding basenames across directories (`_2`, `_3`, …).
pub(super) fn register_media(media: &mut BTreeMap<String, String>, part: String) -> String {
    if let Some(existing) = media.get(&part) {
        return existing.clone();
    }
    let base = sanitize_media_basename(&part);
    let candidate = if !media.values().any(|v| v == &base) {
        base
    } else {
        let (stem, ext) = match base.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (base, String::new()),
        };
        let mut n = 2usize;
        loop {
            let cand = format!("{stem}_{n}{ext}");
            if !media.values().any(|v| v == &cand) {
                break cand;
            }
            n += 1;
        }
    };
    media.insert(part, candidate.clone());
    candidate
}

fn sanitize_media_basename(part: &str) -> String {
    let raw = part.rsplit(['/', '\\']).next().unwrap_or("");
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_start_matches('.');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "media_file".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `docProps/core.xml` `dc:title`.
pub(super) fn read_title(zip: &mut zip::ZipArchive<fs::File>) -> Result<Option<String>> {
    let Ok(core) = parse_part(zip, "docProps/core.xml") else {
        return Ok(None);
    };
    Ok(first(&core, "title")
        .and_then(|el| el.get_text())
        .map(|c| c.trim().to_string())
        .filter(|s| !s.is_empty()))
}
