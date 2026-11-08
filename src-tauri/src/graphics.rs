//! The Graphics setting: how sharp the editor draws each photo.
//!
//! Every editor view used to draw the 400 px contact-sheet thumbnail, which a
//! Retina cover panel stretches about 3x, so sharp photos read as blurred.
//! The setting picks a larger preview the sidecar writes on demand
//! (`Previewer.swift`) into `thumbnails/<px>/<hash>.jpg`, where `cache.rs`
//! counts and evicts it with the photo's thumbnail.
//!
//! Measured on an M4 with a Sony ARW: a 1600 px preview reads the RAW's own
//! embedded 1616 px JPEG in about 130 ms; 2400 px needs a full decode, about
//! 500 ms. Export always reads the originals, so this never touches print.

use serde::{Deserialize, Serialize};

/// What the user chose. `Auto` resolves through `auto_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsMode {
    Auto,
    Low,
    Optimised,
    Maximum,
}

/// What the editor actually draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Low,
    Optimised,
    Maximum,
}

impl GraphicsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::Optimised => "optimised",
            Self::Maximum => "maximum",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Self::Auto),
            "low" => Some(Self::Low),
            "optimised" => Some(Self::Optimised),
            "maximum" => Some(Self::Maximum),
            _ => None,
        }
    }
}

impl Level {
    /// The preview's long edge, or `None` when the thumbnail is enough.
    pub fn max_pixel(self) -> Option<u32> {
        match self {
            Self::Low => None,
            Self::Optimised => Some(1600),
            Self::Maximum => Some(2400),
        }
    }
}

/// The memory at and above which Auto draws at Maximum. A 16 GB Mac
/// reports exactly this.
const MAXIMUM_FROM_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// What Auto means on a Mac with this much memory, or `None` when the
/// amount could not be read, in which case Auto is not offered at all.
pub fn auto_level(memory_bytes: Option<u64>) -> Option<Level> {
    memory_bytes.map(|bytes| {
        if bytes >= MAXIMUM_FROM_BYTES {
            Level::Maximum
        } else {
            Level::Optimised
        }
    })
}

/// Mirrors `app/types/graphics.ts`, pinned by
/// `tests/fixtures/wire/graphics-status.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsStatus {
    pub mode: GraphicsMode,
    /// What Auto resolves to here; `None` hides the Auto option.
    pub detected: Option<Level>,
    pub effective: Level,
}

/// Resolves the stored choice. Nothing stored means Auto. Auto on a Mac
/// whose memory could not be read is reported as Optimised, because the
/// webview cannot offer an Auto it cannot explain.
pub fn status(stored: Option<GraphicsMode>, detected: Option<Level>) -> GraphicsStatus {
    let mode = match (stored.unwrap_or(GraphicsMode::Auto), detected) {
        (GraphicsMode::Auto, None) => GraphicsMode::Optimised,
        (mode, _) => mode,
    };
    let effective = match mode {
        GraphicsMode::Auto => detected.unwrap_or(Level::Optimised),
        GraphicsMode::Low => Level::Low,
        GraphicsMode::Optimised => Level::Optimised,
        GraphicsMode::Maximum => Level::Maximum,
    };
    GraphicsStatus { mode, detected, effective }
}

/// Physical memory from `sysctl hw.memsize`.
pub fn detect_memory() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: the name is a NUL-terminated literal, and `bytes`/`size`
    // describe a u64-sized buffer that outlives the call.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size == std::mem::size_of::<u64>() && bytes > 0).then_some(bytes)
}

/// A content hash is about to become a file name, so it must be exactly
/// what `hash::content_hash` produces: 64 lowercase hex digits.
pub fn is_content_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// How many photos go to the sidecar per request: small enough that a
/// superseded pass stops within about four seconds of RAW decoding, large
/// enough to amortise the round-trip.
pub const PREVIEW_BATCH: usize = 8;

/// One pass of writing the previews for `items` into `dir`, in order.
///
/// Photos with a bad hash are dropped and repeats sent once. Previews
/// already on disk are emitted first, in one batch, without asking
/// `write`; the rest go to `write` in `batch`-sized chunks, each emitted as
/// it lands. `current` is asked before every chunk, so a pass superseded by
/// a newer one stops between chunks. A `write` error ends the pass: the
/// photos it left keep their thumbnails.
pub fn run_previews(
    items: Vec<crate::protocol::PreviewItem>,
    dir: &std::path::Path,
    batch: usize,
    current: &mut dyn FnMut() -> bool,
    mut write: impl FnMut(Vec<crate::protocol::PreviewItem>) -> Result<Vec<Option<String>>, crate::sidecar::SidecarError>,
    mut emit: impl FnMut(std::collections::HashMap<String, String>),
) {
    let mut seen = std::collections::HashSet::new();
    let mut on_disk = std::collections::HashMap::new();
    let mut missing = Vec::new();
    for item in items {
        if !is_content_hash(&item.hash) || !seen.insert(item.hash.clone()) {
            continue;
        }
        let existing = dir.join(format!("{}.jpg", item.hash));
        if existing.is_file() {
            on_disk.insert(item.hash, existing.to_string_lossy().into_owned());
        } else {
            missing.push(item);
        }
    }
    if !on_disk.is_empty() {
        emit(on_disk);
    }
    for chunk in missing.chunks(batch.max(1)) {
        if !current() {
            return;
        }
        let hashes: Vec<String> = chunk.iter().map(|i| i.hash.clone()).collect();
        match write(chunk.to_vec()) {
            Ok(paths) => {
                let found: std::collections::HashMap<_, _> = hashes
                    .into_iter()
                    .zip(paths)
                    .filter_map(|(hash, path)| Some((hash, path?)))
                    .collect();
                if !found.is_empty() {
                    emit(found);
                }
            }
            Err(err) => {
                log::warn!("graphics: preview pass stopped: {err}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    use crate::protocol::PreviewItem;
    use crate::sidecar::SidecarError;
    use std::collections::HashMap;

    fn item(n: u8) -> PreviewItem {
        PreviewItem { path: format!("/p/{n}.ARW"), hash: format!("{n:0>64}") }
    }

    /// Runs a pass whose sidecar "writes" every path it is given, recording
    /// what it was asked for and what reached the webview.
    fn pass(
        dir: &std::path::Path,
        items: Vec<PreviewItem>,
        batch: usize,
        mut current: impl FnMut() -> bool,
    ) -> (Vec<Vec<String>>, Vec<HashMap<String, String>>) {
        let mut asked = Vec::new();
        let mut emitted = Vec::new();
        run_previews(
            items,
            dir,
            batch,
            &mut current,
            |chunk| {
                asked.push(chunk.iter().map(|i| i.hash.clone()).collect());
                Ok(chunk.iter().map(|i| Some(format!("/written/{}.jpg", i.hash))).collect())
            },
            |found| emitted.push(found),
        );
        (asked, emitted)
    }

    /// Every pass after the first opens the same book, so a photo already
    /// written must cost no sidecar trip, and it must be answered before
    /// any photo that does.
    #[test]
    fn previews_already_on_disk_are_answered_first_without_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("{}.jpg", item(2).hash)), b"x").unwrap();
        let (asked, emitted) = pass(dir.path(), vec![item(1), item(2), item(3)], 8, || true);

        assert_eq!(asked, vec![vec![item(1).hash, item(3).hash]]);
        let on_disk = dir.path().join(format!("{}.jpg", item(2).hash));
        assert_eq!(emitted[0], HashMap::from([(item(2).hash, on_disk.to_string_lossy().into_owned())]));
        assert_eq!(emitted[1].len(), 2);
    }

    /// A hash becomes a file name, and a photo placed twice in a book is one
    /// preview, not two writes of the same file.
    #[test]
    fn a_bad_hash_is_never_sent_and_a_repeated_photo_is_sent_once() {
        let dir = tempfile::tempdir().unwrap();
        let bad = PreviewItem { path: "/p/x.jpg".into(), hash: "../../escape".into() };
        let (asked, _) = pass(dir.path(), vec![item(1), bad, item(1), item(2)], 8, || true);
        assert_eq!(asked, vec![vec![item(1).hash, item(2).hash]]);
    }

    /// Book order is kept across batches, so the cover is sharpened first.
    #[test]
    fn previews_are_written_in_book_order_in_batches() {
        let dir = tempfile::tempdir().unwrap();
        let (asked, emitted) = pass(dir.path(), (1..=5).map(item).collect(), 2, || true);
        let hashes = |ns: &[u8]| ns.iter().map(|&n| item(n).hash).collect::<Vec<_>>();
        assert_eq!(asked, vec![hashes(&[1, 2]), hashes(&[3, 4]), hashes(&[5])]);
        assert_eq!(emitted.len(), 3);
    }

    /// Changing the setting mid-pass starts another; this one must stop
    /// between batches rather than keep the sidecar busy with the old size.
    #[test]
    fn a_superseded_pass_stops_between_batches() {
        let dir = tempfile::tempdir().unwrap();
        let mut checks = 0;
        let (asked, _) = pass(dir.path(), (1..=6).map(item).collect(), 2, || {
            checks += 1;
            checks <= 1
        });
        assert_eq!(asked.len(), 1);
    }

    /// An unreadable photo answers null: it keeps its thumbnail and is not
    /// reported as a preview.
    #[test]
    fn a_photo_the_sidecar_could_not_read_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut emitted = Vec::new();
        run_previews(
            vec![item(1), item(2)],
            dir.path(),
            8,
            &mut || true,
            |_| Ok(vec![None, Some("/written/2.jpg".into())]),
            |found| emitted.push(found),
        );
        assert_eq!(emitted, vec![HashMap::from([(item(2).hash, "/written/2.jpg".to_string())])]);
    }

    /// A sidecar that cannot answer at all would fail every later batch the
    /// same way, so the pass ends there.
    #[test]
    fn a_sidecar_failure_ends_the_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut calls = 0;
        run_previews(
            (1..=6).map(item).collect(),
            dir.path(),
            2,
            &mut || true,
            |_| {
                calls += 1;
                Err(SidecarError::Closed)
            },
            |_| {},
        );
        assert_eq!(calls, 1);
    }

    /// The boundary itself and one byte under it, so a `>` for `>=` (which
    /// would put every 16 GB Mac on Optimised) fails.
    #[test]
    fn auto_draws_at_maximum_from_sixteen_gigabytes() {
        assert_eq!(auto_level(Some(16 * GIB)), Some(Level::Maximum));
        assert_eq!(auto_level(Some(16 * GIB - 1)), Some(Level::Optimised));
        assert_eq!(auto_level(Some(8 * GIB)), Some(Level::Optimised));
        assert_eq!(auto_level(Some(64 * GIB)), Some(Level::Maximum));
        assert_eq!(auto_level(None), None);
    }

    #[test]
    fn nothing_stored_means_auto_on_a_mac_it_can_read() {
        let s = status(None, Some(Level::Maximum));
        assert_eq!(s.mode, GraphicsMode::Auto);
        assert_eq!(s.effective, Level::Maximum);
    }

    #[test]
    fn auto_on_a_mac_it_cannot_read_is_optimised() {
        for stored in [None, Some(GraphicsMode::Auto)] {
            let s = status(stored, None);
            assert_eq!(s.mode, GraphicsMode::Optimised, "{stored:?}");
            assert_eq!(s.effective, Level::Optimised, "{stored:?}");
        }
    }

    /// A chosen level wins over detection in both directions.
    #[test]
    fn a_chosen_level_ignores_what_the_mac_could_handle() {
        assert_eq!(status(Some(GraphicsMode::Low), Some(Level::Maximum)).effective, Level::Low);
        assert_eq!(status(Some(GraphicsMode::Maximum), Some(Level::Optimised)).effective, Level::Maximum);
        assert_eq!(status(Some(GraphicsMode::Maximum), None).effective, Level::Maximum);
    }

    #[test]
    fn low_writes_no_previews_and_the_others_write_their_own_size() {
        assert_eq!(Level::Low.max_pixel(), None);
        assert_eq!(Level::Optimised.max_pixel(), Some(1600));
        assert_eq!(Level::Maximum.max_pixel(), Some(2400));
    }

    #[test]
    fn every_mode_round_trips_through_its_stored_text() {
        for mode in [GraphicsMode::Auto, GraphicsMode::Low, GraphicsMode::Optimised, GraphicsMode::Maximum] {
            assert_eq!(GraphicsMode::parse(mode.as_str()), Some(mode));
            assert_eq!(serde_json::to_value(mode).unwrap(), mode.as_str());
        }
        assert_eq!(GraphicsMode::parse("ultra"), None);
    }

    #[test]
    fn this_mac_reports_its_memory() {
        let bytes = detect_memory().expect("hw.memsize is readable on every Mac");
        assert!(bytes >= 4 * GIB, "{bytes}");
    }

    #[test]
    fn only_a_sixty_four_digit_lowercase_hex_hash_names_a_file() {
        let good = "a".repeat(64);
        assert!(is_content_hash(&good));
        assert!(is_content_hash(&"0123456789abcdef".repeat(4)));
        assert!(!is_content_hash(&"A".repeat(64)));
        assert!(!is_content_hash(&"a".repeat(63)));
        assert!(!is_content_hash(&format!("../{}", "a".repeat(61))));
        assert!(!is_content_hash(""));
    }
}
