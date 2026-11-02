use crate::{ExtractorTrial, OUTPUT_LIMIT, Result};
use quick_xml::{Reader as XmlReader, events::Event};
use serde_json::json;
use std::{fs::File, io::Read, path::Path};
use zip::ZipArchive;

fn archive(path: &Path) -> Result<ZipArchive<File>> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    if zip.len() > 4096 {
        return Err("extraction_limit".into());
    }
    let mut total = 0u64;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        total = total.checked_add(entry.size()).ok_or("extraction_limit")?;
        if total > 64 * 1024 * 1024
            || entry.size() > 32 * 1024 * 1024
            || entry.size() > entry.compressed_size().max(1) * 100
        {
            return Err("extraction_limit".into());
        }
    }
    Ok(zip)
}

fn xml_text(xml: &str, properties: bool) -> Result<Vec<(String, String)>> {
    let mut reader = XmlReader::from_str(xml);
    let mut stack: Vec<(String, String)> = Vec::new();
    let mut values = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                stack.push((e.local_name().as_ref().to_owned(), String::new()));
                if stack.len() > 128 {
                    return Err("extraction_limit".into());
                }
            }
            Event::End(_) => {
                if let Some((name, text)) = stack.pop()
                    && (properties || name == "t")
                    && !text.is_empty()
                {
                    values.push((name, text));
                }
            }
            Event::Text(e) => {
                if let Some((_, text)) = stack.last_mut() {
                    text.push_str(&e.xml_content(quick_xml::XmlVersion::Explicit1_0));
                }
            }
            Event::GeneralRef(e) => {
                if let Some((_, text)) = stack.last_mut() {
                    text.push_str(&quick_xml::escape::unescape(&format!("&{};", e.as_ref()))?);
                }
            }
            Event::DocType(_) => return Err("extraction_failed: DTD forbidden".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err("extraction_failed: incomplete XML".into());
    }
    Ok(values)
}

fn read_xml(zip: &mut ZipArchive<File>, name: &str) -> Result<String> {
    let mut xml = String::new();
    zip.by_name(name)?.read_to_string(&mut xml)?;
    Ok(xml)
}

fn properties(zip: &mut ZipArchive<File>, trial: &mut ExtractorTrial) -> Result<()> {
    if zip
        .file_names()
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .any(|name| name == "docProps/core.xml")
    {
        for (key, value) in xml_text(&read_xml(zip, "docProps/core.xml")?, true)? {
            trial.metadata.insert(key, json!(value));
        }
    }
    Ok(())
}

fn append(trial: &mut ExtractorTrial, text: &str) -> Result<()> {
    if trial.text.len() + text.len() + 1 > OUTPUT_LIMIT {
        return Err("extraction_limit".into());
    }
    trial.text.push_str(text);
    trial.text.push('\n');
    Ok(())
}

pub(crate) fn document(path: &Path, format: &str) -> Result<ExtractorTrial> {
    let mut zip = archive(path)?;
    let mut trial = ExtractorTrial::default();
    properties(&mut zip, &mut trial)?;
    let mut names: Vec<String> = if format == "docx" {
        vec!["word/document.xml".into()]
    } else {
        zip.file_names()
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|name| {
                (name.starts_with("ppt/slides/slide")
                    || name.starts_with("ppt/notesSlides/notesSlide"))
                    && name.ends_with(".xml")
                    && !name.contains("/_rels/")
            })
            .map(|name| name.into_owned())
            .collect()
    };
    names.sort_by_key(|name| {
        let number: String = name.chars().filter(char::is_ascii_digit).collect();
        (
            name.contains("notesSlides"),
            number.parse::<u32>().unwrap_or(0),
        )
    });
    let notes = names
        .iter()
        .filter(|name| name.contains("notesSlides"))
        .count();
    if format == "pptx" {
        trial.metadata.insert("notes_pages".into(), json!(notes));
    }
    for name in names {
        for (_, value) in xml_text(&read_xml(&mut zip, &name)?, false)? {
            append(&mut trial, &value)?;
        }
    }
    Ok(trial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_decodes_entities_and_rejects_truncation_and_dtd() -> Result<()> {
        let values = xml_text("<w:p xmlns:w='w'><w:t>A &amp; B</w:t></w:p>", false)?;
        assert_eq!(
            values.into_iter().map(|(_, v)| v).collect::<String>(),
            "A & B"
        );
        assert!(xml_text("<p><t>unfinished", false).is_err());
        assert!(xml_text("<!DOCTYPE a><a/>", false).is_err());
        Ok(())
    }
}
