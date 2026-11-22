//! Editing a media file's XMP metadata and writing it back (M6.396; Encore's Metadata panel edits the main
//! properties and saves them with the file).
//!
//! Only the properties edited change: the packet the file has (its sidecar's, else the one in the file) keeps every
//! other property as it is written, byte for byte; the edited ones are taken out (as attributes or elements of the
//! top-level `rdf:Description`s) and written again as elements of the first one, each declaring its namespaces. The
//! packet goes where Adobe's XMP Specification Part 3 puts it: into a sidecar the file already has; into a PNG
//! (an uncompressed `iTXt` chunk `XML:com.adobe.xmp` after `IHDR`), a JPEG (an `APP1` segment
//! `http://ns.adobe.com/xap/1.0/` after the other application segments) or an MP4 (a top-level `uuid` box
//! BE7ACFCB-97A9-42E8-9C71-999491E3AFAC, written at the end of the file, so the boxes before it and the offsets into
//! them stay); into a new sidecar (`name.xmp`, Adobe's name, or `name.ext.xmp` when another file of the folder has
//! that base name) for any other file, for a JPEG packet too large for one segment, and for an MP4 whose last box
//! runs to the end of the file. A PNG or JPEG is written whole to a temporary file that then takes its place; an MP4
//! (often large) is changed in place: a packet that fits where the old one was is written over it, else the old box
//! becomes `free` and the new one goes at the end.

use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::metadata::{MAX_PACKET, packet_of};
use crate::xmp_schemas::{PropertyKind, PropertySpec, SCHEMAS, SchemaSpec};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// The largest PNG or JPEG rewritten to take a packet.
const MAX_REWRITE: u64 = 512 << 20;
/// The longest value taken, characters.
const MAX_VALUE: usize = 8000;
/// The most items of a list (creators, keywords).
const MAX_ITEMS: usize = 500;
/// Padding left in a packet so it can grow where it is (XMP Part 1, 7.3.2).
const PADDING: usize = 2048;
/// The XMP box's UUID in an MP4 (XMP Part 3, MPEG-4).
const MP4_XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];
/// The header of a JPEG's standard XMP segment, and of an extended one.
const JPEG_XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const JPEG_EXTENDED_XMP: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
/// A PNG's XMP keyword.
const PNG_XMP: &[u8] = b"XML:com.adobe.xmp\0";

/// An editable property of [`crate::xmp_schemas::SCHEMAS`] (M6.397: any the Metadata panel edits, as Encore's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditableProperty {
    /// Its schema.
    pub schema: &'static SchemaSpec,
    /// The property.
    pub property: &'static PropertySpec,
}

impl EditableProperty {
    /// Every editable property, in the panel's order.
    pub fn all() -> impl Iterator<Item = EditableProperty> {
        SCHEMAS.iter().flat_map(|schema| {
            schema
                .properties
                .iter()
                .filter(|property| property.kind.editable())
                .map(move |property| EditableProperty { schema, property })
        })
    }

    /// The name in commands: the schema's prefix and the property's local name (`dc:title`, `xmp:Rating`).
    #[must_use]
    pub fn key(self) -> String {
        format!("{}:{}", self.schema.prefix, self.property.local)
    }

    /// The editable property named `key` in commands.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::all().find(|property| property.key() == key)
    }

    /// Its namespace, prefix and local name.
    fn qualified(self) -> (&'static str, &'static str, &'static str) {
        (
            self.schema.namespace,
            self.schema.prefix,
            self.property.local,
        )
    }

    fn kind(self) -> Kind {
        match self.property.kind {
            PropertyKind::LanguageAlternative => Kind::LanguageAlternative,
            PropertyKind::Ordered => Kind::Ordered,
            PropertyKind::Unordered => Kind::Unordered,
            _ => Kind::Simple,
        }
    }
}

/// How a property's value is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    LanguageAlternative,
    Ordered,
    Unordered,
    Simple,
}

/// Where [`write_metadata`] put the packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Written {
    /// Into the media file itself.
    File,
    /// Into this sidecar file.
    Sidecar(PathBuf),
}

/// The values of the editable properties of the file at `path` (a list's items joined by "; "; a language
/// alternative's default, else its first): each one the packet has.
#[must_use]
pub fn editable_values(path: &Path) -> Vec<(EditableProperty, String)> {
    let (_, packet) = packet_of(path);
    packet
        .and_then(|packet| String::from_utf8(packet).ok())
        .map(|text| values_in(&text))
        .unwrap_or_default()
}

/// The editable properties' values in the packet `text`.
fn values_in(text: &str) -> Vec<(EditableProperty, String)> {
    let Ok(document) = roxmltree::Document::parse(text) else {
        return Vec::new();
    };
    let descriptions = top_descriptions(&document);
    let mut out = Vec::new();
    for field in EditableProperty::all() {
        let (namespace, _, local) = field.qualified();
        let value = descriptions.iter().find_map(|description| {
            if let Some(value) = description.attribute((namespace, local)) {
                return Some(value.to_owned());
            }
            let element = description
                .children()
                .find(|child| child.has_tag_name((namespace, local)))?;
            Some(element_value(element, field.kind()))
        });
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            out.push((field, value));
        }
    }
    out
}

/// A property element's value as the panel shows it.
fn element_value(element: roxmltree::Node<'_, '_>, kind: Kind) -> String {
    let items: Vec<roxmltree::Node<'_, '_>> = element
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "li")))
        .collect();
    if items.is_empty() {
        return element.text().unwrap_or_default().trim().to_owned();
    }
    let text = |node: &roxmltree::Node<'_, '_>| node.text().unwrap_or_default().trim().to_owned();
    match kind {
        Kind::LanguageAlternative => items
            .iter()
            .find(|item| {
                item.attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                    == Some("x-default")
            })
            .or(items.first())
            .map(text)
            .unwrap_or_default(),
        _ => items
            .iter()
            .map(text)
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// The `rdf:Description`s directly under `rdf:RDF`: the properties of the file itself.
fn top_descriptions<'a, 'input>(
    document: &'a roxmltree::Document<'input>,
) -> Vec<roxmltree::Node<'a, 'input>> {
    document
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "Description")))
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.has_tag_name((RDF, "RDF")))
        })
        .collect()
}

/// A packet with nothing in it.
const EMPTY_PACKET: &str = concat!(
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">",
    "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
    "<rdf:Description rdf:about=\"\"/>",
    "</rdf:RDF></x:xmpmeta>"
);

/// Sets `edits` (an empty value removes the property) in the XMP metadata of the file at `path` and writes it back
/// (see the module's notes for where).
///
/// # Errors
///
/// When the file's packet cannot be read as XMP, or the file or a sidecar cannot be written.
pub fn write_metadata(
    path: &Path,
    edits: &[(EditableProperty, String)],
) -> Result<Written, String> {
    let (sidecar, packet) = packet_of(path);
    let current = match packet {
        Some(bytes) => String::from_utf8(bytes)
            .map_err(|_| "the file's XMP metadata is not UTF-8".to_string())?,
        None => EMPTY_PACKET.to_owned(),
    };
    let edited = edit_packet(&current, edits).ok_or_else(|| {
        "the file's XMP metadata cannot be edited (not RDF as expected)".to_string()
    })?;
    if let Some(sidecar) = sidecar {
        write_replacing(&sidecar, wrap_packet(&edited, PADDING).as_bytes())?;
        return Ok(Written::Sidecar(sidecar));
    }
    let head = read_head(path, 16).unwrap_or_default();
    let embedded = if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        rewrite(path, |bytes| {
            embed_png(bytes, wrap_packet(&edited, PADDING).as_bytes())
        })?
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        rewrite(path, |bytes| {
            embed_jpeg(bytes, wrap_packet(&edited, PADDING).as_bytes())
        })?
    } else if head.get(4..8) == Some(&b"ftyp"[..]) && head.get(8..12) != Some(&b"qt  "[..]) {
        embed_mp4(path, &edited)?
    } else {
        false
    };
    if embedded {
        return Ok(Written::File);
    }
    let sidecar = new_sidecar(path);
    write_replacing(&sidecar, wrap_packet(&edited, PADDING).as_bytes())?;
    Ok(Written::Sidecar(sidecar))
}

/// The packet `text` (`<x:xmpmeta …>…</x:xmpmeta>`) with `edits` made; `None` when it is not XMP's RDF.
fn edit_packet(text: &str, edits: &[(EditableProperty, String)]) -> Option<String> {
    let document = roxmltree::Document::parse(text).ok()?;
    let descriptions = top_descriptions(&document);
    let first = descriptions.first()?;
    // (range, replacement), not overlapping.
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for (field, _) in edits {
        let (namespace, _, local) = field.qualified();
        for description in &descriptions {
            for attribute in description.attributes() {
                if attribute.namespace() == Some(namespace) && attribute.name() == local {
                    changes.push((attribute.range(), String::new()));
                }
            }
            for child in description.children() {
                if child.has_tag_name((namespace, local)) {
                    changes.push((child.range(), String::new()));
                }
            }
        }
    }
    let added: String = edits
        .iter()
        .filter_map(|(field, value)| property_xml(*field, value))
        .collect();
    let range = first.range();
    let element = text.get(range.clone())?;
    if element.ends_with("/>") {
        // `<rdf:Description …/>`: opened, the properties, closed with its own name.
        let name: String = element
            .get(1..)?
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
            .collect();
        changes.push((
            range.end.checked_sub(2)?..range.end,
            format!(">{added}</{name}>"),
        ));
    } else {
        let close = range.start.checked_add(element.rfind("</")?)?;
        changes.push((close..close, added));
    }
    changes.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut out = text.to_owned();
    let mut last_start = usize::MAX;
    for (range, replacement) in changes {
        // Ranges taken twice (a property in two descriptions is found once each) or overlapping are skipped.
        if range.end > last_start || range.start > out.len() || range.end > out.len() {
            continue;
        }
        out.replace_range(range.clone(), &replacement);
        last_start = range.start;
    }
    roxmltree::Document::parse(&out).ok()?;
    (out.len() <= MAX_PACKET).then_some(out)
}

/// A property element for `field` holding `value` (`None` for an empty value: the property is removed). Lists take
/// items separated by ";".
fn property_xml(field: EditableProperty, value: &str) -> Option<String> {
    let value: String = value.trim().chars().take(MAX_VALUE).collect();
    if value.is_empty() {
        return None;
    }
    let (namespace, prefix, local) = field.qualified();
    let open = format!("<{prefix}:{local} xmlns:{prefix}=\"{namespace}\" xmlns:rdf=\"{RDF}\">");
    let close = format!("</{prefix}:{local}>");
    let body = match field.kind() {
        Kind::Simple if field.property.kind == PropertyKind::Boolean => {
            if value.eq_ignore_ascii_case("true") {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Kind::Simple => escape(&value),
        Kind::LanguageAlternative => format!(
            "<rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt>",
            escape(&value)
        ),
        Kind::Ordered | Kind::Unordered => {
            let list = if field.kind() == Kind::Ordered {
                "Seq"
            } else {
                "Bag"
            };
            let items: String = value
                .split(';')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .take(MAX_ITEMS)
                .map(|item| format!("<rdf:li>{}</rdf:li>", escape(item)))
                .collect();
            if items.is_empty() {
                return None;
            }
            format!("<rdf:{list}>{items}</rdf:{list}>")
        }
    };
    Some(format!("{open}{body}{close}"))
}

/// `text` with XML's special characters escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters other than tab and line ends cannot be in XML.
            c if c.is_control() && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// `xmpmeta` in an XMP packet wrapper (XMP Part 1, 7.3) with `padding` bytes to grow into.
fn wrap_packet(xmpmeta: &str, padding: usize) -> String {
    let mut out = String::with_capacity(xmpmeta.len().saturating_add(padding).saturating_add(80));
    out.push_str("<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str(xmpmeta);
    out.push('\n');
    let mut left = padding;
    while left > 0 {
        let line = left.min(100);
        out.extend(std::iter::repeat_n(' ', line.saturating_sub(1)));
        out.push('\n');
        left -= line;
    }
    out.push_str("<?xpacket end=\"w\"?>");
    out
}

/// The first `count` bytes of the file at `path`.
fn read_head(path: &Path, count: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let _ = std::fs::File::open(path)
        .ok()?
        .take(count)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// Writes `bytes` to `path` through a temporary file beside it that then takes its place, so a failure leaves the
/// old file.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".disccraft-tmp");
    let temporary = path.with_file_name(name);
    let error = |error: std::io::Error| format!("{}: {error}", path.display());
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(problem) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error(problem));
    }
    std::fs::rename(&temporary, path).map_err(|problem| {
        let _ = std::fs::remove_file(&temporary);
        error(problem)
    })
}

/// Rewrites the file at `path` with `make` (from its bytes); `false` when `make` declines (the packet then goes to a
/// sidecar).
fn rewrite(path: &Path, make: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Result<bool, String> {
    let length = std::fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if length > MAX_REWRITE {
        return Ok(false);
    }
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    match make(&bytes) {
        Some(out) => write_replacing(path, &out).map(|()| true),
        None => Ok(false),
    }
}

/// The sidecar a file without one gets: `name.xmp` (Adobe's), or `name.ext.xmp` when another file of the folder
/// has the base name `name`.
fn new_sidecar(path: &Path) -> PathBuf {
    let plain = path.with_extension("xmp");
    let stem = path.file_stem().map(std::ffi::OsStr::to_os_string);
    let shared = path
        .parent()
        .and_then(|folder| std::fs::read_dir(folder).ok())
        .is_some_and(|entries| {
            entries.flatten().any(|entry| {
                let other = entry.path();
                other != path
                    && other
                        .extension()
                        .is_none_or(|ext| !ext.eq_ignore_ascii_case("xmp"))
                    && other.file_stem().map(std::ffi::OsStr::to_os_string) == stem
            })
        });
    if !shared && plain != path {
        return plain;
    }
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".xmp");
    path.with_file_name(name)
}

/// The CRC-32 of a PNG chunk (ISO 3309, as PNG 5.5 uses it) over `parts`.
fn crc32(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for byte in *part {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
    }
    !crc
}

/// A PNG (`bytes`) with `packet` as its XMP: an uncompressed `iTXt` chunk after `IHDR`, any other XMP chunk left
/// out. `None` when the file is not a whole PNG.
fn embed_png(bytes: &[u8], packet: &[u8]) -> Option<Vec<u8>> {
    let signature = bytes.get(..8)?;
    let mut out = Vec::with_capacity(bytes.len().checked_add(packet.len())?.checked_add(64)?);
    out.extend_from_slice(signature);
    let mut data = Vec::with_capacity(packet.len().checked_add(32)?);
    data.extend_from_slice(PNG_XMP);
    // Not compressed, method 0, no language, no translated keyword.
    data.extend_from_slice(&[0, 0, 0, 0]);
    data.extend_from_slice(packet);
    let length = u32::try_from(data.len()).ok()?;
    let mut at = 8usize;
    let mut placed = false;
    let mut ended = false;
    while at < bytes.len() {
        let length_bytes: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        let size = usize::try_from(u32::from_be_bytes(length_bytes)).ok()?;
        let end = at.checked_add(12)?.checked_add(size)?;
        let chunk = bytes.get(at..end)?;
        let kind = chunk.get(4..8)?;
        let body = chunk.get(8..8usize.checked_add(size)?)?;
        let ours = kind == b"iTXt" && body.starts_with(PNG_XMP);
        if !ours {
            out.extend_from_slice(chunk);
        }
        if kind == b"IHDR" && !placed {
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(b"iTXt");
            out.extend_from_slice(&data);
            out.extend_from_slice(&crc32(&[b"iTXt", &data]).to_be_bytes());
            placed = true;
        }
        at = end;
        if kind == b"IEND" {
            ended = true;
            break;
        }
    }
    (placed && ended).then_some(out)
}

/// A JPEG (`bytes`) with `packet` as its XMP: an `APP1` segment after the other application segments, any other
/// XMP segment (standard or extended) left out. `None` when the packet does not fit in one segment or the file is
/// not a JPEG as expected.
fn embed_jpeg(bytes: &[u8], packet: &[u8]) -> Option<Vec<u8>> {
    if bytes.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    // The segment's length counts itself (two bytes), the header and the packet.
    let length = u16::try_from(JPEG_XMP.len().checked_add(packet.len())?.checked_add(2)?).ok()?;
    let mut out = Vec::with_capacity(bytes.len().checked_add(packet.len())?.checked_add(64)?);
    out.extend_from_slice(&[0xFF, 0xD8]);
    let mut at = 2usize;
    loop {
        let marker = bytes.get(at..at.checked_add(2)?)?;
        if marker.first() != Some(&0xFF) {
            return None;
        }
        let code = *marker.get(1)?;
        if (0xE0..=0xEF).contains(&code) {
            let size_bytes: [u8; 2] = bytes
                .get(at.checked_add(2)?..at.checked_add(4)?)?
                .try_into()
                .ok()?;
            let size = usize::from(u16::from_be_bytes(size_bytes));
            let end = at.checked_add(2)?.checked_add(size)?;
            let segment = bytes.get(at..end)?;
            let payload = segment.get(4..)?;
            let xmp = code == 0xE1
                && (payload.starts_with(JPEG_XMP) || payload.starts_with(JPEG_EXTENDED_XMP));
            if !xmp {
                out.extend_from_slice(segment);
            }
            at = end;
            continue;
        }
        // The first segment that is not an application segment: the packet goes before it.
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(JPEG_XMP);
        out.extend_from_slice(packet);
        out.extend_from_slice(bytes.get(at..)?);
        return Some(out);
    }
}

/// One top-level box of an MP4: where it starts, its size, its type and (a `uuid` box) its UUID.
struct TopBox {
    offset: u64,
    size: u64,
    kind: [u8; 4],
    uuid: Option<[u8; 16]>,
}

/// The top-level boxes of the MP4 `file` (`length` bytes); `None` when they do not tile the file (a box running to
/// the end of the file with size 0 is `None` too: nothing can follow it).
fn top_boxes(file: &mut std::fs::File, length: u64) -> Option<Vec<TopBox>> {
    let mut boxes = Vec::new();
    let mut at = 0u64;
    while at < length {
        if boxes.len() > 100_000 {
            return None;
        }
        let _ = file.seek(SeekFrom::Start(at)).ok()?;
        let mut header = [0u8; 32];
        let available = usize::try_from(length.checked_sub(at)?.min(32)).ok()?;
        file.read_exact(header.get_mut(..available)?).ok()?;
        let size32 = u32::from_be_bytes(header.get(0..4)?.try_into().ok()?);
        let kind: [u8; 4] = header.get(4..8)?.try_into().ok()?;
        let (size, body) = match size32 {
            0 => return None,
            1 => (
                u64::from_be_bytes(header.get(8..16)?.try_into().ok()?),
                16usize,
            ),
            size => (u64::from(size), 8usize),
        };
        if size < u64::try_from(body).ok()? || at.checked_add(size)? > length {
            return None;
        }
        let uuid = if &kind == b"uuid" {
            header
                .get(body..body.checked_add(16)?)
                .and_then(|uuid| <[u8; 16]>::try_from(uuid).ok())
        } else {
            None
        };
        boxes.push(TopBox {
            offset: at,
            size,
            kind,
            uuid,
        });
        at = at.checked_add(size)?;
    }
    Some(boxes)
}

/// Puts the packet `xmpmeta` into the MP4 at `path` in place (see the module's notes); `false` when the file's boxes
/// cannot take it (the packet then goes to a sidecar).
fn embed_mp4(path: &Path, xmpmeta: &str) -> Result<bool, String> {
    let error = |error: std::io::Error| format!("{}: {error}", path.display());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(error)?;
    let length = file.metadata().map_err(error)?.len();
    let Some(boxes) = top_boxes(&mut file, length) else {
        return Ok(false);
    };
    if boxes.first().is_none_or(|first| &first.kind != b"ftyp") {
        return Ok(false);
    }
    let header = 8u64 + 16;
    let packet = wrap_packet(xmpmeta, PADDING);
    let needed = header.saturating_add(u64::try_from(packet.len()).unwrap_or(u64::MAX));
    let old = boxes.iter().enumerate().find(|(_, top)| {
        top.uuid == Some(MP4_XMP_UUID) && top.size >= header && top.size <= u64::from(u32::MAX)
    });
    let write_box =
        |file: &mut std::fs::File, at: u64, size: u64, packet: &str| -> std::io::Result<()> {
            let size32 = u32::try_from(size).map_err(|_| std::io::Error::other("box too large"))?;
            let _ = file.seek(SeekFrom::Start(at))?;
            file.write_all(&size32.to_be_bytes())?;
            file.write_all(b"uuid")?;
            file.write_all(&MP4_XMP_UUID)?;
            file.write_all(packet.as_bytes())?;
            file.sync_all()
        };
    match old {
        // The old box, wherever it is, takes the new packet padded to its size.
        Some((_, top)) if top.size >= needed => {
            let padding = usize::try_from(top.size - needed)
                .unwrap_or(0)
                .saturating_add(PADDING);
            let packet = wrap_packet(xmpmeta, padding);
            write_box(&mut file, top.offset, top.size, &packet).map_err(error)?;
        }
        // The last box: the new one takes its place and runs on.
        Some((index, top)) if index + 1 == boxes.len() => {
            file.set_len(top.offset).map_err(error)?;
            write_box(&mut file, top.offset, needed, &packet).map_err(error)?;
        }
        // Elsewhere: the old box becomes free space, the new one goes at the end.
        Some((_, top)) => {
            let _ = file
                .seek(SeekFrom::Start(top.offset.saturating_add(4)))
                .map_err(error)?;
            file.write_all(b"free").map_err(error)?;
            write_box(&mut file, length, needed, &packet).map_err(error)?;
        }
        None => write_box(&mut file, length, needed, &packet).map_err(error)?,
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::metadata::file_metadata;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("disccraft-xmp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn p(key: &str) -> EditableProperty {
        EditableProperty::from_key(key).unwrap()
    }

    fn edits() -> Vec<(EditableProperty, String)> {
        vec![
            (p("dc:creator"), "Ann; Bo".to_owned()),
            (p("dc:subject"), "sea; sun".to_owned()),
            (p("dc:title"), "Holiday <2026> & more".to_owned()),
            (p("xmp:Rating"), "4".to_owned()),
        ]
    }

    /// [`edits`] as read back, in the panel's order.
    fn expected() -> Vec<(EditableProperty, String)> {
        edits()
    }

    /// The edited properties replace the old ones (as attributes or elements); every other property stays as it
    /// was written.
    #[test]
    fn an_edit_changes_only_its_properties() {
        let packet = concat!(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">",
            "<rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"1\" ",
            "xmp:CreatorTool=\"Encore\"><dc:title xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><rdf:Alt>",
            "<rdf:li xml:lang=\"x-default\">Old</rdf:li></rdf:Alt></dc:title>",
            "<tiff:Make xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\">Cam</tiff:Make></rdf:Description>",
            "</rdf:RDF></x:xmpmeta>"
        );
        let edited = edit_packet(packet, &edits()).unwrap();
        assert!(edited.contains("xmp:CreatorTool=\"Encore\""), "{edited}");
        assert!(
            edited.contains(
                "<tiff:Make xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\">Cam</tiff:Make>"
            )
        );
        assert!(
            !edited.contains("Old") && !edited.contains("xmp:Rating=\"1\""),
            "{edited}"
        );
        assert_eq!(values_in(&edited), expected());
        // An empty value takes the property out.
        let removed = edit_packet(&edited, &[(p("dc:title"), String::new())]).unwrap();
        assert!(
            values_in(&removed)
                .iter()
                .all(|(field, _)| *field != p("dc:title"))
        );
        // A self-closing description, and a packet that is not RDF.
        let empty = edit_packet(EMPTY_PACKET, &edits()).unwrap();
        assert_eq!(values_in(&empty), expected());
        assert_eq!(
            edit_packet("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"/>", &edits()),
            None
        );
    }

    /// A PNG and a JPEG take the packet into the file; the file reads back with it (and still decodes as a
    /// picture), and a second edit replaces the first packet.
    #[test]
    fn pictures_take_the_packet_into_the_file() {
        let dir = temp_dir("pictures");
        // A 2 x 2 PNG.
        let png = dir.join("still.png");
        {
            let file = std::fs::File::create(&png).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[200; 12]).unwrap();
        }
        assert_eq!(write_metadata(&png, &edits()).unwrap(), Written::File);
        assert_eq!(editable_values(&png), expected());
        assert!(!file_metadata(&png).groups.is_empty());
        let decoded =
            png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png).unwrap()))
                .read_info()
                .unwrap()
                .info()
                .width;
        assert_eq!(decoded, 2);
        write_metadata(&png, &[(p("xmp:Label"), "Red".to_owned())]).unwrap();
        let bytes = std::fs::read(&png).unwrap();
        assert_eq!(
            bytes
                .windows(PNG_XMP.len())
                .filter(|part| *part == PNG_XMP)
                .count(),
            1
        );
        // In the panel's order: the label before the rating.
        let mut more = expected();
        more.insert(3, (p("xmp:Label"), "Red".to_owned()));
        assert_eq!(editable_values(&png), more);

        // A JPEG: start of image, a JFIF segment, then a quantisation table marker and the rest.
        let jpeg = dir.join("still.jpg");
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
        bytes.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        bytes.extend_from_slice(&[0xFF, 0xDB, 0, 4, 0, 0, 0xFF, 0xD9]);
        std::fs::write(&jpeg, &bytes).unwrap();
        assert_eq!(write_metadata(&jpeg, &edits()).unwrap(), Written::File);
        assert_eq!(editable_values(&jpeg), expected());
        let written = std::fs::read(&jpeg).unwrap();
        assert!(written.starts_with(&bytes[..20]) && written.ends_with(&bytes[20..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An MP4 takes the packet in a `uuid` box at its end, the boxes before it unchanged; another edit writes over
    /// it; a QuickTime movie and an unknown file get a sidecar, named after the file unless another file of the
    /// folder has its base name.
    #[test]
    fn movies_and_others_take_the_packet_as_mp4_boxes_or_sidecars() {
        let dir = temp_dir("movies");
        let mp4 = dir.join("clip.mp4");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0, 0, 0, 16]);
        bytes.extend_from_slice(b"ftypisom\0\0\0\0");
        bytes.extend_from_slice(&[0, 0, 0, 12]);
        bytes.extend_from_slice(b"mdat1234");
        bytes.extend_from_slice(&[0, 0, 0, 8]);
        bytes.extend_from_slice(b"moov");
        std::fs::write(&mp4, &bytes).unwrap();
        assert_eq!(write_metadata(&mp4, &edits()).unwrap(), Written::File);
        assert_eq!(editable_values(&mp4), expected());
        let written = std::fs::read(&mp4).unwrap();
        assert!(written.starts_with(&bytes));
        let size = written.len();
        // A small edit fits in the box where it is.
        write_metadata(&mp4, &[(p("xmp:Rating"), "5".to_owned())]).unwrap();
        assert_eq!(std::fs::read(&mp4).unwrap().len(), size);
        assert!(editable_values(&mp4).contains(&(p("xmp:Rating"), "5".to_owned())));

        // QuickTime: a sidecar named after the movie.
        let mov = dir.join("take.mov");
        let mut movie = bytes.clone();
        movie[8..12].copy_from_slice(b"qt  ");
        std::fs::write(&mov, &movie).unwrap();
        assert_eq!(
            write_metadata(&mov, &edits()).unwrap(),
            Written::Sidecar(dir.join("take.xmp"))
        );
        assert_eq!(std::fs::read(&mov).unwrap(), movie);
        assert_eq!(editable_values(&mov), expected());
        // Edited again: the sidecar it has.
        assert_eq!(
            write_metadata(&mov, &[(p("xmp:Label"), "Blue".to_owned())]).unwrap(),
            Written::Sidecar(dir.join("take.xmp"))
        );
        // Two files with one base name: `name.ext.xmp`.
        std::fs::write(dir.join("song.wav"), b"RIFF").unwrap();
        std::fs::write(dir.join("song.mpg"), [0, 0, 1, 0xBA]).unwrap();
        assert_eq!(
            write_metadata(&dir.join("song.mpg"), &edits()).unwrap(),
            Written::Sidecar(dir.join("song.mpg.xmp"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The commands: `asset.setMetadata` writes the fields given into the asset's file and says where,
    /// `asset.getMetadata` reads them back; a rating out of range and an unknown field are refused.
    #[test]
    fn the_commands_write_and_read_an_assets_metadata() {
        let dir = temp_dir("commands");
        let still = dir.join("still.jpg");
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend_from_slice(&[0xFF, 0xDB, 0, 4, 0, 0, 0xFF, 0xD9]);
        std::fs::write(&still, &bytes).unwrap();
        let mut session = crate::Session::new();
        let asset = session.project.add_asset(&still).unwrap();
        let written = session
            .execute(
                "asset.setMetadata",
                &serde_json::json!({ "asset": asset.0, "fields": { "dc:title": "Sea", "dc:subject": "a; b", "xmp:Rating": 3, "xmpRights:Marked": "true" } }),
            )
            .unwrap();
        assert_eq!(written["written"], "file");
        let read = session
            .execute(
                "asset.getMetadata",
                &serde_json::json!({ "asset": asset.0 }),
            )
            .unwrap();
        assert_eq!(
            read["fields"],
            serde_json::json!({ "dc:title": "Sea", "dc:subject": "a; b", "xmp:Rating": "3", "xmpRights:Marked": "True" })
        );
        for fields in [
            serde_json::json!({ "xmp:Rating": "9" }),
            serde_json::json!({ "xmpRights:Marked": "maybe" }),
            serde_json::json!({ "xmpMM:DocumentID": "read only" }),
            serde_json::json!({ "colour": "red" }),
            serde_json::json!({}),
        ] {
            assert!(
                session
                    .execute(
                        "asset.setMetadata",
                        &serde_json::json!({ "asset": asset.0, "fields": fields })
                    )
                    .is_err()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Broken or hostile files neither panic nor get written into.
    #[test]
    fn hostile_files_are_left_alone() {
        let packet = wrap_packet(EMPTY_PACKET, 10);
        for len in 0..40 {
            let bytes: Vec<u8> = [0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13]
                .iter()
                .copied()
                .chain(std::iter::repeat(0xFF))
                .take(len)
                .collect();
            assert_eq!(embed_png(&bytes, packet.as_bytes()), None);
            let jpeg: Vec<u8> = [0xFF, 0xD8, 0xFF, 0xE0, 0xFF, 0xFF]
                .iter()
                .copied()
                .take(len)
                .collect();
            let _ = embed_jpeg(&jpeg, packet.as_bytes());
        }
        // A JPEG packet too large for one segment.
        let huge = vec![b' '; 70_000];
        assert_eq!(embed_jpeg(&[0xFF, 0xD8, 0xFF, 0xDB, 0, 2], &huge), None);
        // An MP4 whose last box runs to the end: a sidecar.
        let dir = temp_dir("hostile");
        let mp4 = dir.join("open.mp4");
        let mut bytes = vec![0, 0, 0, 16];
        bytes.extend_from_slice(b"ftypisom\0\0\0\0");
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.extend_from_slice(b"mdat1234");
        std::fs::write(&mp4, &bytes).unwrap();
        assert_eq!(
            write_metadata(&mp4, &edits()).unwrap(),
            Written::Sidecar(dir.join("open.xmp"))
        );
        assert_eq!(std::fs::read(&mp4).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
