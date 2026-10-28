//! Read the typed-text layer of a reMarkable notebook page.
//!
//! This avoids screenshots entirely: xochitl stores typed text as a v6 .rm
//! "RootText" block. We read the newest page file, concatenate its text items
//! and expose the block geometry so the answer can be typed back in place.

use anyhow::{Context, Result};
use remarkable_lines::v6::block::Block;
use remarkable_lines::v6::scene_item::text::TextItem;
use remarkable_lines::RemarkableFile;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default xochitl document store on the device.
pub const DEFAULT_XOCHITL_DIR: &str = "/home/root/.local/share/remarkable/xochitl";

#[derive(Debug, Clone)]
pub struct PageText {
    /// Text of the page, in document order.
    pub text: String,
    /// Text block origin in page coordinates (1404x1872 for rM2).
    pub x: f64,
    pub y: f64,
    pub width: f32,
    pub rm_path: PathBuf,
}

/// Return the most recently modified .rm file under the document store.
///
/// xochitl rewrites the page file while the page is still open, so the newest
/// file is the page the user is working on.
pub fn find_active_page(dir: &Path) -> Result<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;

    for entry in std::fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let notebook_dir = entry.path();
        if !notebook_dir.is_dir() {
            continue;
        }
        let Ok(inner) = std::fs::read_dir(&notebook_dir) else {
            continue;
        };
        for file in inner.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rm") {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            let Ok(modified) = meta.modified() else { continue };
            if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                best = Some((modified, path));
            }
        }
    }

    best.map(|(_, p)| p)
        .context("no .rm page found in document store")
}

/// Parse a page file and return its typed text plus block geometry.
pub fn read_page_text(path: &Path) -> Result<PageText> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let parsed = RemarkableFile::read(&bytes[..])
        .map_err(|e| anyhow::anyhow!("parse {}: {:?}", path.display(), e))?;

    let blocks = match parsed {
        RemarkableFile::V6 { blocks, .. } => blocks,
        _ => anyhow::bail!("unsupported .rm version in {}", path.display()),
    };

    let mut found = None;
    for block in &blocks {
        if let Block::RootText(root) = block {
            found = Some(&root.text);
            break;
        }
    }
    let text_block = found.context("page has no typed-text layer")?;

    // CrdtSequence is a HashMap; order by CRDT id. Typed text is appended
    // sequentially, so (part2, part1) ordering reproduces document order.
    let mut items: Vec<_> = text_block.items.items.values().collect();
    items.sort_by_key(|i| (i.item_id.part2, i.item_id.part1));

    let mut text = String::new();
    for item in items {
        if let TextItem::Text(s) = &item.value {
            text.push_str(s);
        }
    }

    Ok(PageText {
        text,
        x: text_block.x,
        y: text_block.y,
        width: text_block.width,
        rm_path: path.to_path_buf(),
    })
}

/// Convert a point in page coordinates (1404x1872) to Ghostwriter's virtual
/// 768x1024 space used by the touch simulator.
pub fn page_to_virtual(x: f64, y: f64, page_w: f64, page_h: f64) -> (i32, i32) {
    let vx = ((page_w / 2.0 + x) * 768.0 / page_w).round() as i32;
    let vy = (y * 1024.0 / page_h).round() as i32;
    (vx.clamp(0, 767), vy.clamp(0, 1023))
}
