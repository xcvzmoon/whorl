use serde::{Deserialize, Serialize};

/// Resource ceilings applied to every extraction subprocess.
///
/// Defaults preserve the approved spike limits. Archive checks count actual
/// inflated bytes. The deployment must also cap aggregate CPU and memory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractionLimits {
    /// Maximum input size; defaults to 20 MiB.
    pub max_input_bytes: u64,
    /// Maximum combined inflated archive bytes; defaults to 64 MiB.
    pub max_archive_bytes: u64,
    /// Maximum inflated member bytes; defaults to 32 MiB.
    pub max_member_bytes: u64,
    /// Maximum member expansion ratio; defaults to 100.
    pub max_archive_ratio: u64,
    /// Maximum archive entries; defaults to 4096.
    pub max_archive_entries: usize,
    /// Maximum XML nesting; defaults to 128.
    pub max_xml_depth: usize,
    /// Maximum PDF pages; defaults to 100.
    pub max_pages: usize,
    /// Maximum presentation slides; defaults to 100.
    pub max_slides: usize,
    /// Maximum workbook sheets; defaults to 256.
    pub max_sheets: usize,
    /// Maximum cells per sheet; defaults to one million.
    pub max_cells: usize,
    /// Maximum pixels per image or rendered page; defaults to 25 million.
    pub max_image_pixels: u64,
    /// Maximum UTF-8 text bytes; defaults to 8 MiB.
    pub max_text_bytes: usize,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 20 * 1024 * 1024,
            max_archive_bytes: 64 * 1024 * 1024,
            max_member_bytes: 32 * 1024 * 1024,
            max_archive_ratio: 100,
            max_archive_entries: 4096,
            max_xml_depth: 128,
            max_pages: 100,
            max_slides: 100,
            max_sheets: 256,
            max_cells: 1_000_000,
            max_image_pixels: 25_000_000,
            max_text_bytes: 8 * 1024 * 1024,
        }
    }
}
