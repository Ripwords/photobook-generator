use crate::db::{Db, ANALYZER_VERSION};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// One analysed photo as the cache budget sees it: its features row and its
/// thumbnail, sized together, since evicting one without the other leaves
/// either a re-analysis that keeps a stale thumbnail or a thumbnail nothing
/// will ever show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub hash: String,
    pub bytes: u64,
    pub last_used_at: i64,
    /// A saved book, a draft or an open contact sheet needs this photo.
    /// `resolve_photos` is all or nothing, so evicting one pinned hash
    /// breaks the whole book it belongs to.
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionPlan {
    pub evict: Vec<String>,
    pub bytes_after: u64,
    /// Pinned entries alone exceed the limit. Nothing more can be evicted
    /// without breaking something the user still has.
    pub over_budget: bool,
}

/// Least recently used first, never a pinned entry. A limit of 0 evicts
/// every unpinned entry, which is what "Clear unused" means.
pub fn plan_eviction(entries: &[CacheEntry], limit_bytes: u64) -> EvictionPlan {
    let mut bytes: u64 = entries.iter().map(|e| e.bytes).sum();
    let mut candidates: Vec<&CacheEntry> = entries.iter().filter(|e| !e.pinned).collect();
    candidates.sort_by(|a, b| {
        a.last_used_at
            .cmp(&b.last_used_at)
            .then_with(|| a.hash.cmp(&b.hash))
    });
    let mut evict = Vec::new();
    for entry in candidates {
        if bytes <= limit_bytes {
            break;
        }
        bytes -= entry.bytes;
        evict.push(entry.hash.clone());
    }
    EvictionPlan {
        evict,
        bytes_after: bytes,
        over_budget: bytes > limit_bytes,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheReport {
    pub stale_rows_purged: usize,
    pub orphan_thumbnails_removed: usize,
    pub stamps_removed: usize,
    pub evicted: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub over_budget: bool,
    pub vacuumed: bool,
}

/// Wire type, mirrored by `CacheStatus` in `app/types`. Pinned by
/// `tests/fixtures/wire/cache-status.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStatus {
    pub used_bytes: u64,
    pub pinned_bytes: u64,
    pub limit_bytes: u64,
    pub over_budget: bool,
}

/// Every photo something the user still has depends on, which eviction
/// must never touch: `resolve_photos` is all or nothing, so one evicted hash
/// breaks the whole book it belongs to.
///
/// - Every saved book's photos, trashed ones included, since a trashed book
///   can be restored for 30 days. `purge_deleted` removes its photo list when
///   it is gone for good, and the pin lifts with it.
/// - Every include/exclude decision saved with a book.
/// - Each draft's decisions, and every photo already hashed under one of its
///   folders. A restored draft re-analyses its folders; keeping them cached
///   is what makes that instant rather than a full re-analysis.
/// - `live`: the photos on screen in an open contact sheet.
///
/// Read `live` before calling. A book is saved before its run is forgotten,
/// so reading the runs first and the database second cannot miss a photo
/// that moved from one to the other in between.
pub fn pins(db: &Db, live: impl IntoIterator<Item = String>) -> rusqlite::Result<HashSet<String>> {
    let mut pins: HashSet<String> = live.into_iter().collect();
    for sql in [
        "SELECT hash FROM project_photos",
        "SELECT hash FROM project_photo_overrides",
    ] {
        let mut stmt = db.conn.prepare(sql)?;
        for hash in stmt.query_map([], |r| r.get::<_, String>(0))? {
            pins.insert(hash?);
        }
    }
    let mut under = db.conn.prepare(
        "SELECT hash FROM file_stamps WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2",
    )?;
    for draft in db.list_drafts()? {
        // The draft is the webview's record. One this cannot read pins
        // nothing, rather than failing every enforcement after it.
        let Ok(draft) = serde_json::from_str::<serde_json::Value>(&draft) else {
            continue;
        };
        if let Some(overrides) = draft["overrides"].as_object() {
            pins.extend(overrides.keys().cloned());
        }
        let folders = draft["folders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| f.as_str());
        for folder in folders {
            let folder = folder.trim_end_matches('/');
            for hash in under.query_map([folder.to_string(), format!("{folder}/")], |r| {
                r.get::<_, String>(0)
            })? {
                pins.insert(hash?);
            }
        }
    }
    Ok(pins)
}

/// Brings the cache within `limit` bytes, and always clears what is garbage
/// regardless of the limit: rows from an older analyzer (never served
/// again), thumbnails with neither a row nor a pin, and file stamps naming a
/// hash that is no longer cached. Converges: a second run changes nothing.
///
/// The caller must hold the cache gate exclusively. The sidecar writes a
/// thumbnail before its row exists, so running beside an analysis would
/// delete that thumbnail as an orphan.
pub fn enforce(
    db: &Db,
    thumb_dir: &Path,
    pins: &HashSet<String>,
    limit: u64,
) -> rusqlite::Result<CacheReport> {
    let bytes_before = measure(db, thumb_dir, pins)?.0;

    let stale_rows_purged = db.conn.execute(
        "DELETE FROM features WHERE analyzer_version != ?1",
        [ANALYZER_VERSION],
    )?;

    let rows = current_rows(db)?;
    let mut thumbs = thumbnails(thumb_dir);
    let orphans: Vec<String> = thumbs
        .keys()
        .filter(|h| !rows.contains_key(*h) && !pins.contains(*h))
        .cloned()
        .collect();
    let orphan_thumbnails_removed = remove_thumbnails(thumb_dir, &orphans);
    for hash in &orphans {
        thumbs.remove(hash);
    }

    let mut entries: HashMap<&str, CacheEntry> = HashMap::new();
    for (hash, &(bytes, last_used_at)) in &rows {
        entries.insert(
            hash,
            CacheEntry {
                hash: hash.clone(),
                bytes,
                last_used_at,
                pinned: pins.contains(hash),
            },
        );
    }
    for (hash, &bytes) in &thumbs {
        entries
            .entry(hash)
            .or_insert_with(|| CacheEntry {
                hash: hash.clone(),
                bytes: 0,
                last_used_at: 0,
                pinned: pins.contains(hash),
            })
            .bytes += bytes;
    }
    let entries: Vec<CacheEntry> = entries.into_values().collect();
    let plan = plan_eviction(&entries, limit);

    let tx = db.conn.unchecked_transaction()?;
    {
        let mut delete = tx.prepare("DELETE FROM features WHERE hash = ?1")?;
        for hash in &plan.evict {
            delete.execute([hash])?;
        }
    }
    tx.commit()?;
    remove_thumbnails(thumb_dir, &plan.evict);

    let stamps_removed = remove_stamps(db, pins)?;
    let vacuumed = reclaim(db)?;

    Ok(CacheReport {
        stale_rows_purged,
        orphan_thumbnails_removed,
        stamps_removed,
        evicted: plan.evict.len(),
        bytes_before,
        bytes_after: measure(db, thumb_dir, pins)?.0,
        over_budget: plan.over_budget,
        vacuumed,
    })
}

/// File stamps naming a hash that is no longer cached. A pinned hash keeps
/// its stamp even without a row: re-analysing that book then skips hashing
/// every photo again, and a stamp is a hundred-odd bytes.
fn remove_stamps(db: &Db, pins: &HashSet<String>) -> rusqlite::Result<usize> {
    let tx = db.conn.unchecked_transaction()?;
    let dangling: Vec<(String, String)> = tx
        .prepare(
            "SELECT path, hash FROM file_stamps WHERE hash NOT IN (SELECT hash FROM features)",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut removed = 0;
    {
        let mut delete = tx.prepare("DELETE FROM file_stamps WHERE path = ?1")?;
        for (path, hash) in &dangling {
            if !pins.contains(hash) {
                removed += delete.execute([path])?;
            }
        }
    }
    tx.commit()?;
    Ok(removed)
}

/// What the cache holds now, without changing anything: current rows and
/// every thumbnail on disk, orphans included, since they take space until
/// the next enforcement removes them.
pub fn status(
    db: &Db,
    thumb_dir: &Path,
    pins: &HashSet<String>,
    limit: u64,
) -> rusqlite::Result<CacheStatus> {
    let (used_bytes, pinned_bytes) = measure(db, thumb_dir, pins)?;
    Ok(CacheStatus {
        used_bytes,
        pinned_bytes,
        limit_bytes: limit,
        over_budget: used_bytes > limit,
    })
}

/// Free pages SQLite holds after a delete before a VACUUM is worth rewriting
/// the whole file. Below this, the next analysis reuses the free pages
/// anyway, so the waste is bounded by this constant.
const VACUUM_MIN_FREE_BYTES: i64 = 4 * 1024 * 1024;

fn reclaim(db: &Db) -> rusqlite::Result<bool> {
    let free_pages: i64 = db
        .conn
        .query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let page_size: i64 = db.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    if free_pages * page_size < VACUUM_MIN_FREE_BYTES {
        return Ok(false);
    }
    // Another connection reading or writing makes VACUUM fail with
    // SQLITE_BUSY. The free pages stay reusable, and the next enforcement
    // tries again.
    if let Err(err) = db
        .conn
        .execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")
    {
        log::warn!("cache: VACUUM skipped: {err}");
        return Ok(false);
    }
    Ok(true)
}

fn measure(db: &Db, thumb_dir: &Path, pins: &HashSet<String>) -> rusqlite::Result<(u64, u64)> {
    let rows = current_rows(db)?;
    let thumbs = thumbnails(thumb_dir);
    let sized = rows
        .iter()
        .map(|(h, &(bytes, _))| (h, bytes))
        .chain(thumbs.iter().map(|(h, &b)| (h, b)));
    let (mut used, mut pinned) = (0, 0);
    for (hash, bytes) in sized {
        used += bytes;
        if pins.contains(hash) {
            pinned += bytes;
        }
    }
    Ok((used, pinned))
}

/// Current-version rows: hash to (JSON bytes, last used).
fn current_rows(db: &Db) -> rusqlite::Result<HashMap<String, (u64, i64)>> {
    let mut stmt = db.conn.prepare(
        "SELECT hash, length(CAST(json AS BLOB)), COALESCE(last_used_at, created_at)
         FROM features WHERE analyzer_version = ?1",
    )?;
    let rows = stmt
        .query_map([ANALYZER_VERSION], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)? as u64, r.get(2)?),
            ))
        })?
        .collect();
    rows
}

/// Thumbnails by hash. Only `<64 lowercase hex>.jpg` files are the sidecar's;
/// anything else in the directory is left alone.
fn thumbnails(dir: &Path) -> HashMap<String, u64> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("cache: cannot list thumbnails in {dir:?}: {err}");
            }
            return HashMap::new();
        }
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let hash = name.strip_suffix(".jpg")?;
            let is_hash = hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            let meta = entry.metadata().ok().filter(|m| m.is_file())?;
            is_hash.then(|| (hash.to_string(), meta.len()))
        })
        .collect()
}

fn remove_thumbnails(dir: &Path, hashes: &[String]) -> usize {
    hashes
        .iter()
        .filter(
            |hash| match std::fs::remove_file(dir.join(format!("{hash}.jpg"))) {
                Ok(()) => true,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
                Err(err) => {
                    log::warn!("cache: cannot remove thumbnail {hash}: {err}");
                    false
                }
            },
        )
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_spec::pixajoy_spec;

    fn entry(hash: &str, bytes: u64, last_used_at: i64, pinned: bool) -> CacheEntry {
        CacheEntry {
            hash: hash.into(),
            bytes,
            last_used_at,
            pinned,
        }
    }

    #[test]
    fn plan_under_the_limit_evicts_nothing() {
        let entries = [entry("a", 10, 1, false), entry("b", 20, 2, false)];
        let plan = plan_eviction(&entries, 30);
        assert_eq!(
            plan,
            EvictionPlan {
                evict: vec![],
                bytes_after: 30,
                over_budget: false
            }
        );
    }

    #[test]
    fn plan_evicts_the_least_recently_used_unpinned_entries_first() {
        let entries = [
            entry("newest", 10, 300, false),
            entry("oldest", 10, 100, false),
            entry("middle", 10, 200, false),
        ];
        let plan = plan_eviction(&entries, 15);
        assert_eq!(plan.evict, vec!["oldest".to_string(), "middle".to_string()]);
        assert_eq!(plan.bytes_after, 10);
        assert!(!plan.over_budget);
    }

    #[test]
    fn plan_breaks_a_last_used_tie_by_hash() {
        let entries = [
            entry("b", 10, 5, false),
            entry("a", 10, 5, false),
            entry("c", 10, 9, false),
        ];
        assert_eq!(plan_eviction(&entries, 20).evict, vec!["a".to_string()]);
    }

    #[test]
    fn plan_never_evicts_a_pinned_entry_and_reports_over_budget() {
        let entries = [
            entry("book-photo", 100, 1, true),
            entry("stray", 10, 50, false),
            entry("draft-photo", 100, 2, true),
        ];
        let plan = plan_eviction(&entries, 150);
        assert_eq!(plan.evict, vec!["stray".to_string()]);
        assert_eq!(plan.bytes_after, 200);
        assert!(plan.over_budget);
    }

    #[test]
    fn plan_with_a_zero_limit_evicts_every_unpinned_entry() {
        let entries = [
            entry("a", 1, 3, false),
            entry("p", 1, 1, true),
            entry("b", 1, 2, false),
        ];
        let plan = plan_eviction(&entries, 0);
        assert_eq!(plan.evict, vec!["b".to_string(), "a".to_string()]);
        assert_eq!(plan.bytes_after, 1);
        assert!(plan.over_budget);
    }

    fn h(n: u32) -> String {
        format!("{n:064x}")
    }

    fn thumb(dir: &Path, name: &str, bytes: usize) {
        std::fs::write(dir.join(name), vec![0u8; bytes]).unwrap();
    }

    fn put(db: &Db, hash: &str, json_bytes: usize, last_used_at: i64) {
        db.put_features(hash, "/p.jpg", &"x".repeat(json_bytes))
            .unwrap();
        db.conn
            .execute(
                "UPDATE features SET last_used_at = ?1 WHERE hash = ?2",
                rusqlite::params![last_used_at, hash],
            )
            .unwrap();
    }

    fn put_stale(db: &Db, hash: &str) {
        db.conn
            .execute(
                "INSERT INTO features (hash, path, json, analyzer_version) VALUES (?1, '/p.jpg', '{}', ?2)",
                rusqlite::params![hash, ANALYZER_VERSION - 1],
            )
            .unwrap();
    }

    fn rows(db: &Db) -> Vec<String> {
        db.conn
            .prepare("SELECT hash FROM features ORDER BY hash")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn pinned(hashes: &[&str]) -> HashSet<String> {
        hashes.iter().map(|h| h.to_string()).collect()
    }

    #[test]
    fn enforce_purges_stale_version_rows_and_their_unpinned_thumbnails() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (stale, stale_pinned, fresh) = (h(1), h(2), h(3));
        put_stale(&db, &stale);
        put_stale(&db, &stale_pinned);
        put(&db, &fresh, 10, 1);
        for hash in [&stale, &stale_pinned, &fresh] {
            thumb(dir.path(), &format!("{hash}.jpg"), 5);
        }

        let report = enforce(&db, dir.path(), &pinned(&[&stale_pinned]), u64::MAX).unwrap();

        assert_eq!(report.stale_rows_purged, 2);
        assert_eq!(rows(&db), vec![fresh.clone()]);
        assert_eq!(
            files(dir.path()),
            vec![format!("{stale_pinned}.jpg"), format!("{fresh}.jpg")],
            "a pinned photo keeps its thumbnail so re-analysing its book reuses it"
        );
    }

    #[test]
    fn enforce_removes_orphan_thumbnails_and_leaves_other_files_alone() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (kept, orphan) = (h(1), h(2));
        put(&db, &kept, 10, 1);
        let others = [
            "notes.txt".to_string(),
            format!("{}.jpg", h(0xabc).to_uppercase()),
            format!("{}.jpg", &h(4)[1..]),
            format!("{}.jpeg", h(5)),
        ];
        for name in others
            .iter()
            .chain([&format!("{kept}.jpg"), &format!("{orphan}.jpg")])
        {
            thumb(dir.path(), name, 5);
        }

        let report = enforce(&db, dir.path(), &HashSet::new(), u64::MAX).unwrap();

        assert_eq!(report.orphan_thumbnails_removed, 1);
        assert!(!dir.path().join(format!("{orphan}.jpg")).exists());
        assert!(dir.path().join(format!("{kept}.jpg")).exists());
        for name in &others {
            assert!(
                dir.path().join(name).exists(),
                "{name} is not a thumbnail and must be left alone"
            );
        }
    }

    #[test]
    fn enforce_removes_file_stamps_whose_photo_left_the_cache() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        put(&db, &h(1), 10, 1);
        put_stale(&db, &h(2));
        let stamp = crate::db::FileStamp {
            size: 1,
            modified_ns: 1,
        };
        put_stale(&db, &h(4));
        db.put_stamps(&[
            ("/a.jpg", stamp, &h(1)),
            ("/b.jpg", stamp, &h(2)),
            ("/c.jpg", stamp, &h(3)),
            ("/book.jpg", stamp, &h(4)),
        ])
        .unwrap();

        let report = enforce(&db, dir.path(), &pinned(&[&h(4)]), u64::MAX).unwrap();

        assert_eq!(report.stamps_removed, 2);
        assert!(db.get_stamp("/a.jpg").unwrap().is_some());
        assert!(db.get_stamp("/b.jpg").unwrap().is_none());
        assert!(db.get_stamp("/c.jpg").unwrap().is_none());
        assert!(
            db.get_stamp("/book.jpg").unwrap().is_some(),
            "re-analysing a saved book whose rows went stale skips hashing its photos again"
        );
    }

    #[test]
    fn enforce_evicts_the_least_recently_used_unpinned_photos_with_their_thumbnails() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (old, recent, book) = (h(1), h(2), h(3));
        put(&db, &old, 100, 10);
        put(&db, &recent, 100, 20);
        put(&db, &book, 100, 1);
        for hash in [&old, &recent, &book] {
            thumb(dir.path(), &format!("{hash}.jpg"), 50);
        }

        let report = enforce(&db, dir.path(), &pinned(&[&book]), 300).unwrap();

        assert_eq!(report.evicted, 1);
        assert_eq!((report.bytes_before, report.bytes_after), (450, 300));
        assert!(!report.over_budget);
        let mut expected = vec![recent.clone(), book.clone()];
        expected.sort();
        assert_eq!(rows(&db), expected);
        assert!(!dir.path().join(format!("{old}.jpg")).exists());

        let again = enforce(&db, dir.path(), &pinned(&[&book]), 300).unwrap();
        assert_eq!(
            again,
            CacheReport {
                bytes_before: 300,
                bytes_after: 300,
                ..CacheReport::default()
            },
            "a second run converges and does nothing"
        );
    }

    #[test]
    fn enforce_reports_over_budget_when_pinned_photos_alone_exceed_the_limit() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        put(&db, &h(1), 100, 1);
        put(&db, &h(2), 100, 2);

        let report = enforce(&db, dir.path(), &pinned(&[&h(1)]), 50).unwrap();

        assert_eq!(report.evicted, 1);
        assert!(report.over_budget);
        assert_eq!(rows(&db), vec![h(1)]);
    }

    #[test]
    fn enforce_tolerates_a_thumbnail_directory_that_does_not_exist_yet() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        put(&db, &h(1), 100, 1);
        let report = enforce(&db, &dir.path().join("thumbnails"), &HashSet::new(), 0).unwrap();
        assert_eq!(report.evicted, 1);
    }

    #[test]
    fn status_measures_without_deleting_anything() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        put(&db, &h(1), 100, 1);
        put(&db, &h(2), 40, 1);
        put_stale(&db, &h(3));
        thumb(dir.path(), &format!("{}.jpg", h(1)), 30);
        thumb(dir.path(), &format!("{}.jpg", h(9)), 7);

        let status = status(&db, dir.path(), &pinned(&[&h(1)]), 150).unwrap();

        assert_eq!(
            status,
            CacheStatus {
                used_bytes: 177,
                pinned_bytes: 130,
                limit_bytes: 150,
                over_budget: true
            }
        );
        assert_eq!(rows(&db).len(), 3);
        assert_eq!(files(dir.path()).len(), 2);
    }

    #[test]
    fn pins_cover_every_book_including_trashed_ones_their_overrides_drafts_and_open_runs() {
        use crate::book::cull::{Override, Overrides};
        let db = Db::open_in_memory().unwrap();
        let book = crate::book::pace::Book {
            spec: pixajoy_spec(),
            controls: Default::default(),
            seed: 1,
            dropped: 0,
            pages: vec![],
        };
        let mut overrides = Overrides::new();
        overrides.set("book-override", Override::Exclude);
        db.save_project("Live", "/p", &book, &["live-photo".into()], &overrides)
            .unwrap();
        let trashed = db
            .save_project(
                "Trashed",
                "/p",
                &book,
                &["trashed-photo".into()],
                &Overrides::new(),
            )
            .unwrap();
        db.delete_project(trashed).unwrap();
        db.save_draft(
            1,
            r#"{"id":1,"name":"Kyoto","folders":["/photos/kyoto/"],"replacing":null,"overrides":{"draft-override":"include"}}"#,
        )
        .unwrap();
        db.save_draft(2, "not json").unwrap();
        db.save_draft(3, r#"{"folders":"not a list","overrides":[]}"#)
            .unwrap();
        let stamp = crate::db::FileStamp {
            size: 1,
            modified_ns: 1,
        };
        db.put_stamps(&[
            ("/photos/kyoto/a.jpg", stamp, "in-draft-folder"),
            ("/photos/kyoto/day2/b.jpg", stamp, "in-draft-subfolder"),
            ("/photos/kyoto2/c.jpg", stamp, "in-a-sibling-folder"),
        ])
        .unwrap();

        let pins = pins(&db, ["on-screen".to_string()]).unwrap();

        let mut got: Vec<&str> = pins.iter().map(String::as_str).collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                "book-override",
                "draft-override",
                "in-draft-folder",
                "in-draft-subfolder",
                "live-photo",
                "on-screen",
                "trashed-photo",
            ]
        );
    }

    /// Runs enforcement against a COPY of a real database and thumbnail
    /// directory and reports what it did, and whether every saved book
    /// (trashed ones included) still resolves before and after:
    ///
    /// PBG_CACHE_DB=<copy>/photobook.sqlite PBG_CACHE_THUMBS=<copy>/thumbnails \
    /// PBG_CACHE_LIMIT=20000000 cargo test --lib real_cache -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_cache_enforcement_report() {
        let db_path = std::path::PathBuf::from(std::env::var("PBG_CACHE_DB").unwrap());
        let thumbs = std::path::PathBuf::from(std::env::var("PBG_CACHE_THUMBS").unwrap());
        let limit: u64 = std::env::var("PBG_CACHE_LIMIT").unwrap().parse().unwrap();
        assert!(
            !db_path.starts_with(dirs_home().join("Library/Application Support")),
            "run this against a copy, never the app's own data"
        );

        let db = Db::open(&db_path).unwrap();
        let pins = pins(&db, Vec::new()).unwrap();
        println!("pinned hashes: {}", pins.len());
        snapshot("before", &db, &db_path, &thumbs, &pins, limit);

        if std::env::var("PBG_CACHE_TOUCH").is_ok() {
            measure_touch(&db);
        }
        let unpinned_last_used: HashMap<String, i64> = current_rows(&db)
            .unwrap()
            .into_iter()
            .filter(|(hash, _)| !pins.contains(hash))
            .map(|(hash, (_, last_used))| (hash, last_used))
            .collect();

        let started = std::time::Instant::now();
        let report = enforce(&db, &thumbs, &pins, limit).unwrap();
        println!("enforce({limit}) in {:.2?}: {report:?}", started.elapsed());
        let survivors = current_rows(&db).unwrap();
        let (evicted, kept): (Vec<_>, Vec<_>) = unpinned_last_used
            .iter()
            .partition(|(hash, _)| !survivors.contains_key(*hash));
        println!(
            "unpinned: {} evicted (last used {:?}..{:?}), {} kept (last used {:?}..{:?})",
            evicted.len(),
            evicted.iter().map(|(_, t)| **t).min(),
            evicted.iter().map(|(_, t)| **t).max(),
            kept.len(),
            kept.iter().map(|(_, t)| **t).min(),
            kept.iter().map(|(_, t)| **t).max(),
        );
        snapshot("after", &db, &db_path, &thumbs, &pins, limit);

        let started = std::time::Instant::now();
        let again = enforce(&db, &thumbs, &pins, limit).unwrap();
        println!("second enforce in {:.2?}: {again:?}", started.elapsed());
    }

    /// The touch's cost on the real cache-hit path: every stamped file
    /// looked up in the same ramped chunks an analysis uses, with the touch
    /// each chunk does timed again on its own.
    fn measure_touch(db: &Db) {
        let paths: Vec<String> = db
            .conn
            .prepare("SELECT path FROM file_stamps ORDER BY path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let chunks = crate::sidecar::chunk_paths_ramped(&paths);
        let (mut lookup, mut touch, mut hits) =
            (std::time::Duration::ZERO, std::time::Duration::ZERO, 0);
        for chunk in &chunks {
            let started = std::time::Instant::now();
            let found = crate::commands::lookup_cache(db, chunk).unwrap();
            lookup += started.elapsed();
            let hashes: Vec<&str> = found
                .hits
                .iter()
                .filter_map(|f| f["hash"].as_str())
                .collect();
            hits += hashes.len();
            let started = std::time::Instant::now();
            db.touch_features(&hashes).unwrap();
            touch += started.elapsed();
        }
        println!(
            "touch: {} paths in {} chunks, {hits} hits; lookup_cache total {lookup:.2?} (touch included), \
             the touches alone {touch:.2?}",
            paths.len(),
            chunks.len()
        );
    }

    fn dirs_home() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap())
    }

    fn snapshot(
        label: &str,
        db: &Db,
        db_path: &Path,
        thumbs: &Path,
        pins: &HashSet<String>,
        limit: u64,
    ) {
        let file = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
        let wal = db_path.with_extension("sqlite-wal");
        let thumb_bytes: u64 = thumbnails(thumbs).values().sum();
        println!(
            "[{label}] sqlite {} B (+wal {} B), thumbnails {} files {} B",
            file(db_path),
            file(&wal),
            thumbnails(thumbs).len(),
            thumb_bytes
        );
        let by_version: Vec<(i64, i64)> = db
            .conn
            .prepare("SELECT analyzer_version, count(*) FROM features GROUP BY analyzer_version")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let stamps: i64 = db
            .conn
            .query_row("SELECT count(*) FROM file_stamps", [], |r| r.get(0))
            .unwrap();
        let free: i64 = db
            .conn
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))
            .unwrap();
        println!("[{label}] rows by analyzer_version {by_version:?}, file_stamps {stamps}, freelist pages {free}");
        println!("[{label}] {:?}", status(db, thumbs, pins, limit).unwrap());
        let projects: Vec<(i64, String, Option<i64>)> = db
            .conn
            .prepare("SELECT id, name, deleted_at FROM projects ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for (id, name, deleted_at) in projects {
            let hashes: Vec<String> = db
                .conn
                .prepare("SELECT hash FROM project_photos WHERE project_id = ?1 ORDER BY position")
                .unwrap()
                .query_map([id], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            let resolved = crate::commands::resolve_photos(db, &hashes);
            println!(
                "[{label}] project {id} {name:?}{} ({} photos): {}",
                if deleted_at.is_some() {
                    " (trashed)"
                } else {
                    ""
                },
                hashes.len(),
                match resolved {
                    Ok(photos) => format!("Ok, {} photos", photos.len()),
                    Err(err) => format!("Err: {err}"),
                }
            );
        }
    }
}
