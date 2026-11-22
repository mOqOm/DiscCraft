//! The XMP metadata of a media file, for the Metadata panel (Encore: Window ▸ Metadata, "Powered By XMP").
//!
//! XMP is an RDF/XML packet (ISO 16684-1; Adobe's XMP Specification Part 1 for the data model and Part 3 for
//! where a format keeps it). Whatever the format, the packet lies in the file as plain UTF-8 text between
//! `<x:xmpmeta` and `</x:xmpmeta>`, so it is found by looking for those; a sidecar file (`name.xmp` beside the
//! media, or `name.ext.xmp`) is read first. Writing back is [`crate::metadata_edit`]'s (M6.396).
//!
//! Everything read is the file's and so untrusted: the search is bounded, the packet's size and the number and
//! length of the values are capped, and the XML parser takes no DTD.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the head of a file is searched for a packet.
const HEAD_BYTES: u64 = 8 << 20;
/// How much of the tail of a file is searched (formats that put the packet last).
const TAIL_BYTES: u64 = 2 << 20;
/// The largest packet read.
pub(crate) const MAX_PACKET: usize = 4 << 20;
/// The most properties shown of one file.
const MAX_ITEMS: usize = 2000;
/// The longest value shown, characters.
const MAX_VALUE: usize = 2000;
/// How deep structures inside a property are followed.
const MAX_DEPTH: usize = 6;

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

/// The properties of one schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataGroup {
    /// The schema's name as Encore's Metadata panel lists it ("Dublin Core", [`crate::xmp_schemas::SCHEMAS`]), or its
    /// namespace when it is none of those.
    pub name: String,
    /// The schema's namespace.
    pub namespace: String,
    /// The properties: name and value, in the packet's order.
    pub items: Vec<(String, String)>,
}

/// A file's XMP metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    /// The sidecar file the packet was read from; `None` when it was in the file itself (or there is none).
    pub sidecar: Option<PathBuf>,
    /// The schemas that have properties, in the order Encore lists them, others after.
    pub groups: Vec<MetadataGroup>,
}

/// The XMP metadata of the file at `path`: its sidecar's, else the packet in the file. Empty when the file has
/// none (or cannot be read).
#[must_use]
pub fn file_metadata(path: &Path) -> Metadata {
    let (sidecar, packet) = packet_of(path);
    let groups = packet
        .and_then(|bytes| parse_packet(&bytes))
        .unwrap_or_default();
    Metadata { sidecar, groups }
}

/// The XMP packet of the file at `path` (`<x:xmpmeta …>…</x:xmpmeta>`): its sidecar's (and the sidecar), else the
/// one in the file; `None` when it has none.
pub(crate) fn packet_of(path: &Path) -> (Option<PathBuf>, Option<Vec<u8>>) {
    for sidecar in sidecars(path) {
        if let Some(packet) = std::fs::read(&sidecar)
            .ok()
            .filter(|bytes| bytes.len() <= MAX_PACKET)
            .and_then(|bytes| packet_in(&bytes).map(<[u8]>::to_vec))
            .filter(|packet| parse_packet(packet).is_some())
        {
            return (Some(sidecar), Some(packet));
        }
    }
    (None, embedded_packet(path).or_else(|| png_packet(path)))
}

/// Where a sidecar of `path` would be: `movie.xmp` and `movie.mp4.xmp`.
fn sidecars(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.with_extension("xmp")];
    if let Some(name) = path.file_name() {
        let mut with = name.to_os_string();
        with.push(".xmp");
        out.push(path.with_file_name(with));
    }
    out.retain(|sidecar| sidecar != path && sidecar.is_file());
    out
}

/// The packet inside `bytes`: from `<x:xmpmeta` to the end of `</x:xmpmeta>`.
fn packet_in(bytes: &[u8]) -> Option<&[u8]> {
    const OPEN: &[u8] = b"<x:xmpmeta";
    const CLOSE: &[u8] = b"</x:xmpmeta>";
    let start = bytes.windows(OPEN.len()).position(|part| part == OPEN)?;
    let rest = bytes.get(start..)?;
    let end = rest.windows(CLOSE.len()).position(|part| part == CLOSE)?;
    let end = end.checked_add(CLOSE.len())?;
    rest.get(..end).filter(|packet| packet.len() <= MAX_PACKET)
}

/// The packet in the file at `path`, looked for in its head and its tail.
fn embedded_packet(path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut read = |from: u64, most: u64| -> Option<Vec<u8>> {
        let _ = file.seek(SeekFrom::Start(from)).ok()?;
        let mut bytes = Vec::new();
        let _ = (&mut file).take(most).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    };
    let head = read(0, HEAD_BYTES)?;
    if let Some(packet) = packet_in(&head) {
        return Some(packet.to_vec());
    }
    if length <= HEAD_BYTES {
        return None;
    }
    let tail = read(
        length.saturating_sub(TAIL_BYTES).max(HEAD_BYTES / 2),
        TAIL_BYTES + HEAD_BYTES,
    )?;
    packet_in(&tail).map(<[u8]>::to_vec)
}

/// The largest picture of a PNG file decoded to reach a packet kept after it.
const MAX_PNG_PICTURE: usize = 256 << 20;

/// The packet of a PNG file kept in a compressed `iTXt` chunk (keyword `XML:com.adobe.xmp`; XMP Specification
/// Part 3, PNG): it is not plain text in the file, so the search for the packet's text does not find it. A chunk
/// after the picture is reached by decoding the picture (up to [`MAX_PNG_PICTURE`]).
fn png_packet(path: &Path) -> Option<Vec<u8>> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 8];
    file.read_exact(&mut head).ok()?;
    if head != SIGNATURE {
        return None;
    }
    let _ = file.seek(SeekFrom::Start(0)).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_ignore_text_chunk(false);
    let mut reader = decoder.read_info().ok()?;
    if let Some(packet) = png_xmp(reader.info()) {
        return Some(packet);
    }
    let size = reader.output_buffer_size()?;
    if size > MAX_PNG_PICTURE {
        return None;
    }
    let mut picture = vec![0u8; size];
    let _ = reader.next_frame(&mut picture).ok()?;
    reader.finish().ok()?;
    png_xmp(reader.info())
}

/// The packet in a PNG file's international text chunks, compressed or not.
fn png_xmp(info: &png::Info<'_>) -> Option<Vec<u8>> {
    info.utf8_text
        .iter()
        .filter(|chunk| chunk.keyword == "XML:com.adobe.xmp")
        .find_map(|chunk| {
            let mut chunk = chunk.clone();
            chunk.decompress_text_with_limit(MAX_PACKET).ok()?;
            let text = chunk.get_text().ok()?;
            packet_in(text.as_bytes()).map(<[u8]>::to_vec)
        })
}

/// The properties of an XMP packet, by schema. `None` when it is not well-formed XML or holds no property.
fn parse_packet(packet: &[u8]) -> Option<Vec<MetadataGroup>> {
    let text = std::str::from_utf8(packet).ok()?;
    let document = roxmltree::Document::parse(text).ok()?;
    // Properties by namespace, in the packet's order.
    let mut found: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut count = 0usize;
    let mut add = |namespace: &str, name: String, value: String| {
        if count >= MAX_ITEMS || value.trim().is_empty() {
            return;
        }
        count += 1;
        let value: String = value.trim().chars().take(MAX_VALUE).collect();
        match found.iter_mut().find(|(known, _)| known == namespace) {
            Some((_, items)) => items.push((name, value)),
            None => found.push((namespace.to_owned(), vec![(name, value)])),
        }
    };
    for description in document
        .descendants()
        .filter(|node| node.has_tag_name((RDF, "Description")))
        // The properties of the resource itself: a nested description is a structure's value.
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.has_tag_name((RDF, "RDF")))
        })
    {
        // Simple properties written as attributes.
        for attribute in description.attributes() {
            if let Some(namespace) = attribute
                .namespace()
                .filter(|namespace| *namespace != RDF && *namespace != XML)
            {
                add(
                    namespace,
                    attribute.name().to_owned(),
                    attribute.value().to_owned(),
                );
            }
        }
        for property in description.children().filter(roxmltree::Node::is_element) {
            let Some(namespace) = property.tag_name().namespace().filter(|ns| *ns != RDF) else {
                continue;
            };
            let mut values = Vec::new();
            value_of(property, "", MAX_DEPTH, &mut values);
            let name = property.tag_name().name();
            for (path, value) in values {
                let name = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name} / {path}")
                };
                add(namespace, name, value);
            }
        }
    }
    if found.is_empty() {
        return None;
    }
    // Encore's schemas first, in its order; the others after, by their namespace.
    let mut groups = Vec::new();
    for schema in crate::xmp_schemas::SCHEMAS {
        if let Some(at) = found
            .iter()
            .position(|(known, _)| known == schema.namespace)
        {
            let (namespace, items) = found.remove(at);
            groups.push(MetadataGroup {
                name: schema.name.to_owned(),
                namespace,
                items,
            });
        }
    }
    groups.extend(found.into_iter().map(|(namespace, items)| MetadataGroup {
        name: namespace.clone(),
        namespace,
        items,
    }));
    Some(groups)
}

/// The value of a property element: its text; the items of an array (`rdf:Seq` / `rdf:Bag` / `rdf:Alt`) joined;
/// the fields of a structure, each under its name (`path` says where, for a field inside a field).
fn value_of(
    node: roxmltree::Node<'_, '_>,
    path: &str,
    depth: usize,
    out: &mut Vec<(String, String)>,
) {
    if depth == 0 {
        return;
    }
    let elements: Vec<roxmltree::Node<'_, '_>> = node
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    // A simple value: the text, or the resource it names.
    if elements.is_empty() {
        let text = node
            .text()
            .map(str::to_owned)
            .or_else(|| node.attribute((RDF, "resource")).map(str::to_owned))
            .unwrap_or_default();
        if !text.trim().is_empty() {
            out.push((path.to_owned(), text));
        }
        // A structure written as attributes of the property element.
        for attribute in node.attributes() {
            if attribute
                .namespace()
                .is_some_and(|namespace| namespace != RDF && namespace != XML)
            {
                out.push((join(path, attribute.name()), attribute.value().to_owned()));
            }
        }
        return;
    }
    for element in elements {
        let is_rdf = element.tag_name().namespace() == Some(RDF);
        match (is_rdf, element.tag_name().name()) {
            // An array: its items one after another.
            (true, "Seq" | "Bag" | "Alt") => {
                let mut simple = Vec::new();
                for (number, item) in element
                    .children()
                    .filter(|item| item.has_tag_name((RDF, "li")))
                    .enumerate()
                {
                    let mut values = Vec::new();
                    value_of(item, "", depth - 1, &mut values);
                    match values.as_slice() {
                        [(inner, value)] if inner.is_empty() => {
                            simple.push(value.trim().to_owned())
                        }
                        _ => {
                            let at = join(path, &format!("{}", number.saturating_add(1)));
                            out.extend(
                                values
                                    .into_iter()
                                    .map(|(inner, value)| (join(&at, &inner), value)),
                            );
                        }
                    }
                }
                if !simple.is_empty() {
                    out.push((path.to_owned(), simple.join("; ")));
                }
            }
            // A structure: a nested description.
            (true, "Description") => {
                for attribute in element.attributes() {
                    if attribute
                        .namespace()
                        .is_some_and(|namespace| namespace != RDF && namespace != XML)
                    {
                        out.push((join(path, attribute.name()), attribute.value().to_owned()));
                    }
                }
                for field in element.children().filter(roxmltree::Node::is_element) {
                    value_of(field, &join(path, field.tag_name().name()), depth - 1, out);
                }
            }
            (true, _) => {}
            // A structure written with `rdf:parseType="Resource"`: its fields are the children.
            (false, name) => value_of(element, &join(path, name), depth - 1, out),
        }
    }
}

/// `path` with `name` under it.
fn join(path: &str, name: &str) -> String {
    match (path.is_empty(), name.is_empty()) {
        (true, _) => name.to_owned(),
        (_, true) => path.to_owned(),
        _ => format!("{path} / {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Metadata, file_metadata, packet_in, parse_packet};

    const PACKET: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmpDM="http://ns.adobe.com/xmp/1.0/DynamicMedia/"
    xmlns:own="http://example.com/own/"
    xmp:CreatorTool="A camera" own:note="mine">
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Holiday</rdf:li></rdf:Alt></dc:title>
   <dc:subject><rdf:Bag><rdf:li>sea</rdf:li><rdf:li>sun</rdf:li></rdf:Bag></dc:subject>
   <xmp:CreateDate>2026-08-01T10:00:00+09:00</xmp:CreateDate>
   <xmpDM:duration xmpDM:value="5400" xmpDM:scale="1/30"/>
   <xmpDM:Tracks><rdf:Bag><rdf:li rdf:parseType="Resource">
     <xmpDM:trackName>Markers</xmpDM:trackName>
     <xmpDM:markers><rdf:Seq><rdf:li rdf:parseType="Resource">
       <xmpDM:startTime>90</xmpDM:startTime><xmpDM:name>Chapter 2</xmpDM:name>
     </rdf:li></rdf:Seq></xmpDM:markers>
   </rdf:li></rdf:Bag></xmpDM:Tracks>
   <dc:empty></dc:empty>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    fn item<'a>(metadata: &'a Metadata, group: &str, name: &str) -> Option<&'a str> {
        metadata
            .groups
            .iter()
            .find(|found| found.name == group)?
            .items
            .iter()
            .find(|(found, _)| found == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn a_packet_is_found_in_a_file_and_read_by_schema() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-xmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        // The packet in the middle of other bytes, as a media file holds it.
        let file = dir.join("movie.mp4");
        let mut bytes = vec![0u8, 1, 2, 0xFF, 0xFE];
        bytes.extend_from_slice(PACKET.as_bytes());
        bytes.extend_from_slice(&[0, 0, 7, 9]);
        std::fs::write(&file, &bytes)?;
        let metadata = file_metadata(&file);
        assert_eq!(metadata.sidecar, None);
        let names: Vec<&str> = metadata
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect();
        // Encore's order, a schema of the file's own last.
        assert_eq!(
            names,
            [
                "Dublin Core",
                "Basic",
                "Dynamic Media",
                "http://example.com/own/"
            ]
        );
        assert_eq!(item(&metadata, "Dublin Core", "title"), Some("Holiday"));
        assert_eq!(item(&metadata, "Dublin Core", "subject"), Some("sea; sun"));
        assert_eq!(item(&metadata, "Dublin Core", "empty"), None);
        assert_eq!(item(&metadata, "Basic", "CreatorTool"), Some("A camera"));
        assert_eq!(
            item(&metadata, "Basic", "CreateDate"),
            Some("2026-08-01T10:00:00+09:00")
        );
        assert_eq!(
            item(&metadata, "Dynamic Media", "duration / value"),
            Some("5400")
        );
        assert_eq!(
            item(
                &metadata,
                "Dynamic Media",
                "Tracks / 1 / markers / 1 / name"
            ),
            Some("Chapter 2")
        );
        assert_eq!(
            item(&metadata, "http://example.com/own/", "note"),
            Some("mine")
        );

        // A sidecar is read before the file.
        let sidecar = dir.join("movie.xmp");
        std::fs::write(&sidecar, PACKET.replace("Holiday", "From the sidecar"))?;
        let metadata = file_metadata(&file);
        assert_eq!(metadata.sidecar.as_deref(), Some(sidecar.as_path()));
        assert_eq!(
            item(&metadata, "Dublin Core", "title"),
            Some("From the sidecar")
        );

        // A file without a packet, and one that is not there, have none.
        let plain = dir.join("plain.bin");
        std::fs::write(&plain, [1u8; 64])?;
        assert_eq!(file_metadata(&plain), Metadata::default());
        assert_eq!(file_metadata(&dir.join("missing.mov")), Metadata::default());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// A PNG file with the packet in a compressed `iTXt` chunk, before or after the picture.
    fn png_with_packet(
        packet: &str,
        after_picture: bool,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 4, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut chunk = png::text_metadata::ITXtChunk::new("XML:com.adobe.xmp", packet);
            chunk.compressed = true;
            chunk.compress_text()?;
            let mut writer = encoder.write_header()?;
            if !after_picture {
                writer.write_text_chunk(&chunk)?;
            }
            writer.write_image_data(&[90u8; 4 * 2 * 3])?;
            if after_picture {
                writer.write_text_chunk(&chunk)?;
            }
            writer.finish()?;
        }
        Ok(bytes)
    }

    #[test]
    fn a_compressed_packet_in_a_png_is_read() -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::env::temp_dir().join(format!("disccraft-xmp-png-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        for after_picture in [false, true] {
            let file = dir.join(format!("menu-{after_picture}.png"));
            let bytes = png_with_packet(PACKET, after_picture)?;
            // Compressed: the packet's text is not in the file.
            assert!(packet_in(&bytes).is_none());
            std::fs::write(&file, &bytes)?;
            let metadata = file_metadata(&file);
            assert_eq!(item(&metadata, "Dublin Core", "title"), Some("Holiday"));
            assert_eq!(item(&metadata, "Basic", "CreatorTool"), Some("A camera"));
        }
        // A broken PNG, and a chunk without a packet, give none.
        let broken = dir.join("broken.png");
        let mut bytes = png_with_packet(PACKET, false)?;
        bytes.truncate(40);
        std::fs::write(&broken, &bytes)?;
        assert_eq!(file_metadata(&broken), Metadata::default());
        let other = dir.join("other.png");
        std::fs::write(&other, png_with_packet("no packet here", false)?)?;
        assert_eq!(file_metadata(&other), Metadata::default());
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn broken_packets_are_not_read() {
        assert!(packet_in(b"<x:xmpmeta no end").is_none());
        assert!(packet_in(b"nothing").is_none());
        assert!(parse_packet(b"<x:xmpmeta></x:xmpmeta>").is_none());
        assert!(parse_packet(b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><oops></x:xmpmeta>").is_none());
        assert!(parse_packet(&[0xFF, 0xFE, 0x00]).is_none());
        // A document type is refused by the parser (no entities are expanded).
        let bomb = b"<!DOCTYPE a [<!ENTITY e 'x'>]><x:xmpmeta xmlns:x='adobe:ns:meta/'/>";
        assert!(parse_packet(bomb).is_none());
    }
}
