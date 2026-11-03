use super::{ExtractedDocument, ExtractionError, ExtractionLimits, Format};
use calamine::{Reader, Xlsx, open_workbook};
use quick_xml::{Reader as XmlReader, events::Event};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};
use zip::ZipArchive;

type Result<T> = std::result::Result<T, ExtractionError>;

#[derive(Default)]
struct Node {
    name: String,
    attributes: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}

fn malformed<T>(_: T) -> ExtractionError {
    ExtractionError::MalformedFile
}

fn xml(bytes: &[u8], limits: &ExtractionLimits) -> Result<Node> {
    let mut reader = XmlReader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut stack = vec![Node::default()];
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Start(e) => {
                let mut node = Node {
                    name: e.local_name().as_ref().to_owned(),
                    ..Node::default()
                };
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(malformed)?;
                    let key = attribute.key.as_ref().to_owned();
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                        .map_err(malformed)?
                        .into_owned();
                    if node.attributes.insert(key, value).is_some() {
                        return Err(ExtractionError::MalformedFile);
                    }
                }
                if stack.len() > limits.max_xml_depth {
                    return Err(ExtractionError::LimitExceeded);
                }
                stack.push(node);
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err(ExtractionError::MalformedFile);
                }
                let node = stack.pop().ok_or(ExtractionError::MalformedFile)?;
                stack
                    .last_mut()
                    .ok_or(ExtractionError::MalformedFile)?
                    .children
                    .push(node);
            }
            Event::Text(e) => stack
                .last_mut()
                .ok_or(ExtractionError::MalformedFile)?
                .text
                .push_str(&e.xml_content(quick_xml::XmlVersion::Explicit1_0)),
            Event::CData(e) => stack
                .last_mut()
                .ok_or(ExtractionError::MalformedFile)?
                .text
                .push_str(&e.xml_content(quick_xml::XmlVersion::Explicit1_0)),
            Event::GeneralRef(e) => {
                let reference = format!("&{};", e.as_ref());
                let text = quick_xml::escape::unescape(&reference).map_err(malformed)?;
                stack
                    .last_mut()
                    .ok_or(ExtractionError::MalformedFile)?
                    .text
                    .push_str(&text);
            }
            Event::DocType(_) => return Err(ExtractionError::MalformedFile),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(ExtractionError::MalformedFile);
    }
    let mut root = stack.pop().ok_or(ExtractionError::MalformedFile)?;
    if root.children.len() != 1 || !root.text.trim().is_empty() {
        return Err(ExtractionError::MalformedFile);
    }
    Ok(root.children.remove(0))
}

struct Package {
    members: BTreeMap<String, Vec<u8>>,
}

impl Package {
    fn open(path: &Path, limits: &ExtractionLimits) -> Result<Self> {
        let file = File::open(path).map_err(|_| ExtractionError::ExtractionFailed)?;
        if file.metadata().map_err(malformed)?.len() > limits.max_input_bytes {
            return Err(ExtractionError::LimitExceeded);
        }
        let mut archive = ZipArchive::new(file).map_err(malformed)?;
        if archive.len() > limits.max_archive_entries {
            return Err(ExtractionError::LimitExceeded);
        }
        let mut members = BTreeMap::new();
        let mut total = 0u64;
        let mut actual_total = 0u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(malformed)?;
            let name = entry.name().map_err(malformed)?.into_owned();
            if name.starts_with('/')
                || name.contains('\\')
                || name.split('/').any(|part| part == "..")
            {
                return Err(ExtractionError::MalformedFile);
            }
            if entry.size() > limits.max_member_bytes
                || entry.size()
                    > entry
                        .compressed_size()
                        .max(1)
                        .saturating_mul(limits.max_archive_ratio)
            {
                return Err(ExtractionError::LimitExceeded);
            }
            total = total
                .checked_add(entry.size())
                .ok_or(ExtractionError::LimitExceeded)?;
            if total > limits.max_archive_bytes {
                return Err(ExtractionError::LimitExceeded);
            }
            let expansion_cap = entry
                .compressed_size()
                .max(1)
                .saturating_mul(limits.max_archive_ratio);
            let mut bytes = Vec::new();
            entry
                .take(limits.max_member_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(malformed)?;
            actual_total = actual_total
                .checked_add(bytes.len() as u64)
                .ok_or(ExtractionError::LimitExceeded)?;
            if bytes.len() as u64 > limits.max_member_bytes
                || bytes.len() as u64 > expansion_cap
                || actual_total > limits.max_archive_bytes
            {
                return Err(ExtractionError::LimitExceeded);
            }
            if name.ends_with(".xml") || name.ends_with(".rels") {
                xml(&bytes, limits)?;
            }
            if members.insert(name, bytes).is_some() {
                return Err(ExtractionError::MalformedFile);
            }
        }
        Ok(Self { members })
    }

    fn xml(&self, name: &str, limits: &ExtractionLimits) -> Result<Node> {
        xml(
            self.members
                .get(name)
                .ok_or(ExtractionError::MalformedFile)?,
            limits,
        )
    }

    fn relations(
        &self,
        source: &str,
        limits: &ExtractionLimits,
    ) -> Result<BTreeMap<String, (String, String)>> {
        let (directory, file) = source.rsplit_once('/').unwrap_or(("", source));
        let name = if source.is_empty() {
            "_rels/.rels".to_owned()
        } else if directory.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{directory}/_rels/{file}.rels")
        };
        if !self.members.contains_key(&name) {
            return Ok(BTreeMap::new());
        }
        let root = self.xml(&name, limits)?;
        if root.name != "Relationships" {
            return Err(ExtractionError::MalformedFile);
        }
        let mut result = BTreeMap::new();
        for node in root.children {
            if node.name != "Relationship" {
                return Err(ExtractionError::MalformedFile);
            }
            if node
                .attributes
                .get("TargetMode")
                .is_some_and(|value| value == "External")
            {
                continue;
            }
            let id = node
                .attributes
                .get("Id")
                .ok_or(ExtractionError::MalformedFile)?
                .clone();
            let kind = node
                .attributes
                .get("Type")
                .ok_or(ExtractionError::MalformedFile)?
                .clone();
            let target = node
                .attributes
                .get("Target")
                .ok_or(ExtractionError::MalformedFile)?;
            let resolved = resolve(directory, target)?;
            if result.insert(id, (kind, resolved)).is_some() {
                return Err(ExtractionError::MalformedFile);
            }
        }
        Ok(result)
    }
}

fn resolve(directory: &str, target: &str) -> Result<String> {
    if target.contains(['\\', ':', '#', '?', '%']) {
        return Err(ExtractionError::MalformedFile);
    }
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        directory.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(ExtractionError::MalformedFile)?;
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(ExtractionError::MalformedFile);
    }
    Ok(parts.join("/"))
}

fn append(output: &mut String, value: &str, limits: &ExtractionLimits) -> Result<()> {
    if output.len().saturating_add(value.len()) > limits.max_text_bytes {
        return Err(ExtractionError::LimitExceeded);
    }
    output.push_str(value);
    Ok(())
}

fn text(node: &Node, output: &mut String, limits: &ExtractionLimits) -> Result<()> {
    if node.name == "t" {
        append(output, &node.text, limits)?;
    }
    if node.name == "tab" {
        append(output, "\t", limits)?;
    }
    if node.name == "br" {
        append(output, "\n", limits)?;
    }
    for child in &node.children {
        text(child, output, limits)?;
    }
    if matches!(node.name.as_str(), "p" | "tr" | "tc") {
        append(output, "\n", limits)?;
    }
    Ok(())
}

fn descendants<'a>(node: &'a Node, name: &str, output: &mut Vec<&'a Node>) {
    if node.name == name {
        output.push(node);
    }
    for child in &node.children {
        descendants(child, name, output);
    }
}

pub(super) fn detect(path: &Path, limits: &ExtractionLimits) -> Result<Format> {
    let package = Package::open(path, limits)?;
    let types = package.xml("[Content_Types].xml", limits)?;
    if types.name != "Types" {
        return Err(ExtractionError::MalformedFile);
    }
    let roots = package.relations("", limits)?;
    let main: Vec<_> = roots
        .values()
        .filter(|(kind, _)| kind.ends_with("/officeDocument"))
        .collect();
    if main.len() != 1 {
        return Err(ExtractionError::MalformedFile);
    }
    let part = format!("/{}", main[0].1);
    let declarations: Vec<_> = types
        .children
        .iter()
        .filter(|node| node.attributes.get("PartName") == Some(&part))
        .collect();
    if declarations.len() != 1 {
        return Err(ExtractionError::MalformedFile);
    }
    match declarations[0]
        .attributes
        .get("ContentType")
        .map(String::as_str)
    {
        Some(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ) => Ok(Format::Docx),
        Some(
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        ) => Ok(Format::Pptx),
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml") => {
            Ok(Format::Xlsx)
        }
        _ => Err(ExtractionError::UnsupportedMediaType),
    }
}

pub(super) fn extract(
    path: &Path,
    format: Format,
    limits: &ExtractionLimits,
) -> Result<ExtractedDocument> {
    let package = Package::open(path, limits)?;
    let content_types = package.xml("[Content_Types].xml", limits)?;
    if content_types.name != "Types" {
        return Err(ExtractionError::MalformedFile);
    }
    let roots = package.relations("", limits)?;
    let main: Vec<_> = roots
        .values()
        .filter(|(kind, _)| kind.ends_with("/officeDocument"))
        .collect();
    if main.len() != 1 {
        return Err(ExtractionError::MalformedFile);
    }
    let main = &main[0].1;
    let expected_type = match format {
        Format::Docx => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
        }
        Format::Pptx => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"
        }
        Format::Xlsx => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
        }
        _ => return Err(ExtractionError::UnsupportedMediaType),
    };
    if !content_types.children.iter().any(|node| {
        node.attributes
            .get("PartName")
            .is_some_and(|name| name == &format!("/{main}"))
            && node
                .attributes
                .get("ContentType")
                .is_some_and(|value| value == expected_type)
    }) {
        return Err(ExtractionError::MalformedFile);
    }
    let mut document = ExtractedDocument {
        text: String::new(),
        metadata: BTreeMap::new(),
        ocr: false,
    };
    for (kind, name) in roots.values() {
        if kind.ends_with("/metadata/core-properties") {
            for property in package.xml(name, limits)?.children {
                if !property.text.is_empty() {
                    document.metadata.insert(property.name, property.text);
                }
            }
        }
    }
    match format {
        Format::Docx => {
            let root = package.xml(main, limits)?;
            if root.name != "document" {
                return Err(ExtractionError::MalformedFile);
            }
            text(&root, &mut document.text, limits)?;
        }
        Format::Pptx => {
            let root = package.xml(main, limits)?;
            if root.name != "presentation" {
                return Err(ExtractionError::MalformedFile);
            }
            let relationships = package.relations(main, limits)?;
            let mut slides = Vec::new();
            descendants(&root, "sldId", &mut slides);
            if slides.len() > limits.max_slides {
                return Err(ExtractionError::LimitExceeded);
            }
            let mut visited = BTreeSet::new();
            let mut notes_count = 0usize;
            for slide in slides {
                let id = slide
                    .attributes
                    .get("r:id")
                    .ok_or(ExtractionError::MalformedFile)?;
                let (kind, name) = relationships
                    .get(id)
                    .ok_or(ExtractionError::MalformedFile)?;
                if !kind.ends_with("/slide") || !visited.insert(name) {
                    return Err(ExtractionError::MalformedFile);
                }
                let root = package.xml(name, limits)?;
                if root.name != "sld" {
                    return Err(ExtractionError::MalformedFile);
                }
                text(&root, &mut document.text, limits)?;
                for (kind, name) in package.relations(name, limits)?.values() {
                    if kind.ends_with("/notesSlide") {
                        notes_count += 1;
                        if notes_count > limits.max_slides {
                            return Err(ExtractionError::LimitExceeded);
                        }
                        text(&package.xml(name, limits)?, &mut document.text, limits)?;
                    }
                }
            }
            document
                .metadata
                .insert("notes_pages".into(), notes_count.to_string());
        }
        Format::Xlsx => {
            let root = package.xml(main, limits)?;
            if root.name != "workbook" {
                return Err(ExtractionError::MalformedFile);
            }
            let mut sheets = Vec::new();
            descendants(&root, "sheet", &mut sheets);
            if sheets.len() > limits.max_sheets {
                return Err(ExtractionError::LimitExceeded);
            }
            let mut workbook: Xlsx<_> = open_workbook(path).map_err(malformed)?;
            let names = workbook.sheet_names();
            if names.len() > limits.max_sheets {
                return Err(ExtractionError::LimitExceeded);
            }
            document
                .metadata
                .insert("sheet_names".into(), names.join("\n"));
            for name in names {
                let range = workbook.worksheet_range(&name).map_err(malformed)?;
                let (rows, columns) = range.get_size();
                if rows.saturating_mul(columns) > limits.max_cells {
                    return Err(ExtractionError::LimitExceeded);
                }
                for row in range.rows() {
                    for cell in row {
                        append(&mut document.text, &cell.to_string(), limits)?;
                        append(&mut document.text, "\t", limits)?;
                    }
                    append(&mut document.text, "\n", limits)?;
                }
            }
        }
        _ => return Err(ExtractionError::UnsupportedMediaType),
    }
    let metadata_bytes = document
        .metadata
        .iter()
        .try_fold(0usize, |total, (key, value)| {
            total.checked_add(key.len())?.checked_add(value.len())
        })
        .ok_or(ExtractionError::LimitExceeded)?;
    if document.text.len().saturating_add(metadata_bytes) > limits.max_text_bytes {
        return Err(ExtractionError::LimitExceeded);
    }
    if document.text.trim().is_empty() {
        return Err(ExtractionError::NoExtractableText);
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_follow_presentation_relationship_order_and_notes() {
        use std::io::{Cursor, Write};
        let members = [
            (
                "[Content_Types].xml",
                "<Types><Override PartName='/ppt/presentation.xml' ContentType='application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml'/></Types>",
            ),
            (
                "_rels/.rels",
                "<Relationships><Relationship Id='main' Type='x/officeDocument' Target='ppt/presentation.xml'/></Relationships>",
            ),
            (
                "ppt/presentation.xml",
                "<p:presentation xmlns:p='p' xmlns:r='r'><p:sldIdLst><p:sldId r:id='first'/><p:sldId r:id='second'/></p:sldIdLst></p:presentation>",
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                "<Relationships><Relationship Id='second' Type='x/slide' Target='slides/slide2.xml'/><Relationship Id='first' Type='x/slide' Target='slides/slide10.xml'/></Relationships>",
            ),
            ("ppt/slides/slide2.xml", "<sld><p><t>second</t></p></sld>"),
            ("ppt/slides/slide10.xml", "<sld><p><t>first</t></p></sld>"),
            (
                "ppt/slides/_rels/slide10.xml.rels",
                "<Relationships><Relationship Id='notes' Type='x/notesSlide' Target='../notesSlides/notesSlide7.xml'/></Relationships>",
            ),
            (
                "ppt/notesSlides/notesSlide7.xml",
                "<notes><p><t>notes</t></p></notes>",
            ),
        ];
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in members {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        let path = std::env::temp_dir().join(format!("whorl-office-{}.pptx", uuid::Uuid::new_v4()));
        std::fs::write(&path, writer.finish().unwrap().into_inner()).unwrap();
        let limits = ExtractionLimits::default();
        let output = extract(&path, Format::Pptx, &limits);
        std::fs::remove_file(path).unwrap();
        let output = output.unwrap();
        assert_eq!(output.text, "first\nnotes\nsecond\n");
        assert_eq!(output.metadata.get("notes_pages").unwrap(), "1");
    }

    #[test]
    fn xml_preserves_runs_and_entities_and_bounds_nesting() {
        let limits = ExtractionLimits::default();
        let root = xml(b"<document><p><r><t>hel</t></r><r><t>lo &amp; bye</t></r><tab/><r><t>world</t></r></p></document>", &limits).unwrap();
        let mut output = String::new();
        text(&root, &mut output, &limits).unwrap();
        assert_eq!(output, "hello & bye\tworld\n");
        for bad in [
            b"<p><t>unfinished".as_slice(),
            b"<!DOCTYPE a><a/>",
            b"<a/><b/>",
            b"<p>&unknown;</p>",
            b"<p><t></p>",
        ] {
            assert!(xml(bad, &limits).is_err());
        }
        let limits = ExtractionLimits {
            max_xml_depth: 1,
            ..limits
        };
        assert!(matches!(
            xml(b"<p><t/></p>", &limits),
            Err(ExtractionError::LimitExceeded)
        ));
    }

    #[test]
    fn corpus_office_formats_and_limits() {
        let limits = ExtractionLimits::default();
        for (name, format) in [
            ("table.docx", Format::Docx),
            ("table.pptx", Format::Pptx),
            ("notes.pptx", Format::Pptx),
            ("workbook.xlsx", Format::Xlsx),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("spikes/extraction/fixtures")
                .join(name);
            assert_eq!(detect(&path, &limits).unwrap(), format);
            let output = extract(&path, format, &limits).unwrap();
            assert!(!output.text.trim().is_empty(), "{name}");
            let tiny = ExtractionLimits {
                max_text_bytes: 1,
                ..limits.clone()
            };
            assert!(matches!(
                extract(&path, format, &tiny),
                Err(ExtractionError::LimitExceeded)
            ));
            let tiny = ExtractionLimits {
                max_archive_bytes: 1,
                ..limits.clone()
            };
            assert!(matches!(
                detect(&path, &tiny),
                Err(ExtractionError::LimitExceeded)
            ));
        }
    }

    #[test]
    fn relationship_paths_cannot_escape_package() {
        assert_eq!(
            resolve("ppt/slides", "../notesSlides/notesSlide1.xml").unwrap(),
            "ppt/notesSlides/notesSlide1.xml"
        );
        assert!(resolve("", "../escape").is_err());
        assert!(resolve("word", "https://example.com/x").is_err());
    }
}
