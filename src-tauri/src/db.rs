use crate::book::cull::{Override, Overrides};
use crate::book::pace::Book;
use crate::project::{self, ExportRecord, Project, ProjectSummary};
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Bump this whenever the Swift analyzer's output changes shape or semantics.
/// Cached rows at an older version are ignored and re-analysed.
///
/// v2 (a9121da): the embedded-preview decode path changed which pixels are
/// analysed -- RAW and HEIC files are now analysed from the camera's
/// embedded preview rather than a full ImageIO demosaic. That shifts
/// aesthetics, sharpness, palette and phash for every RAW/HEIC row, so rows
/// cached under v1 must not be served as if they came from the same
/// pipeline as v2 rows (see raw-performance-report.md for the measured
/// deltas). This is the worked example for what counts as a "semantic
/// change" the next time this constant needs bumping.
pub const ANALYZER_VERSION: u32 = 2;

/// A file's size and modified time. When both match what they were when the
/// file was last hashed, its contents are taken to be unchanged and the hash
/// is reused rather than read again -- the check backup and sync tools make.
/// Re-reading a folder of RAW files on an external drive took over a minute
/// when every photo was already analysed.
///
/// An edit that keeps both the size and the modified time slips past it.
/// Editors and cameras set the modified time on every write, so that takes a
/// tool that deliberately restores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub size: i64,
    pub modified_ns: i64,
}

impl FileStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(Self {
            size: i64::try_from(meta.len()).ok()?,
            modified_ns: i64::try_from(modified.as_nanos()).ok()?,
        })
    }
}

pub struct Db {
    pub(crate) conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let db = Self { conn: Connection::open(path)? };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let db = Self { conn: Connection::open_in_memory()? };
        db.migrate()?;
        Ok(db)
    }

    /// `project_photo_overrides` holds the user's own include/exclude
    /// decisions, keyed by content hash exactly as `Overrides` is. Unordered,
    /// unlike `project_photos`: it is a map, and nothing indexes into it
    /// positionally. `Auto` is never stored -- it is the ABSENCE of a
    /// decision (see `Overrides::set`), so a row for it would be a second
    /// representation of "no decision".
    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS features (
                 hash             TEXT PRIMARY KEY,
                 path             TEXT NOT NULL,
                 json             TEXT NOT NULL,
                 analyzer_version INTEGER NOT NULL,
                 created_at       INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS idx_features_version
                 ON features (analyzer_version);
             CREATE TABLE IF NOT EXISTS projects (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 name          TEXT NOT NULL,
                 source_folder TEXT NOT NULL,
                 page_count    INTEGER NOT NULL,
                 photo_count   INTEGER NOT NULL,
                 book_json     TEXT NOT NULL,
                 created_at    INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at    INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS idx_projects_updated_at
                 ON projects (updated_at);
             CREATE TABLE IF NOT EXISTS project_exports (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 project_id  INTEGER NOT NULL REFERENCES projects(id),
                 output_dir  TEXT NOT NULL,
                 format      TEXT NOT NULL,
                 file_count  INTEGER NOT NULL,
                 exported_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS idx_project_exports_project_id
                 ON project_exports (project_id);
             CREATE TABLE IF NOT EXISTS project_photos (
                 project_id INTEGER NOT NULL REFERENCES projects(id),
                 position   INTEGER NOT NULL,
                 hash       TEXT NOT NULL,
                 PRIMARY KEY (project_id, position)
             );
             CREATE TABLE IF NOT EXISTS project_photo_overrides (
                 project_id INTEGER NOT NULL REFERENCES projects(id),
                 hash       TEXT NOT NULL,
                 state      TEXT NOT NULL,
                 PRIMARY KEY (project_id, hash)
             );
             CREATE TABLE IF NOT EXISTS file_stamps (
                 path        TEXT PRIMARY KEY,
                 size        INTEGER NOT NULL,
                 modified_ns INTEGER NOT NULL,
                 hash        TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS drafts (
                 id         INTEGER PRIMARY KEY,
                 json       TEXT NOT NULL,
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS project_folders (
                 project_id INTEGER NOT NULL REFERENCES projects(id),
                 position   INTEGER NOT NULL,
                 path       TEXT NOT NULL,
                 PRIMARY KEY (project_id, position)
             );",
        )?;
        let has_deleted_at: bool = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'deleted_at'")?
            .exists([])?;
        if !has_deleted_at {
            self.conn.execute("ALTER TABLE projects ADD COLUMN deleted_at INTEGER", [])?;
        }
        let has_favourite: bool = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('projects') WHERE name = 'favourite'")?
            .exists([])?;
        if !has_favourite {
            self.conn
                .execute("ALTER TABLE projects ADD COLUMN favourite INTEGER NOT NULL DEFAULT 0", [])?;
        }
        Ok(())
    }

    pub fn get_features(&self, hash: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT json FROM features WHERE hash = ?1 AND analyzer_version = ?2",
                rusqlite::params![hash, ANALYZER_VERSION],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn put_features(&self, hash: &str, path: &str, json: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO features (hash, path, json, analyzer_version)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hash) DO UPDATE SET
                 path = excluded.path,
                 json = excluded.json,
                 analyzer_version = excluded.analyzer_version",
            rusqlite::params![hash, path, json, ANALYZER_VERSION],
        )?;
        Ok(())
    }

    /// The content hash last computed for `path`, with the size and modified
    /// time the file had then. See `FileStamp`.
    pub fn get_stamp(&self, path: &str) -> rusqlite::Result<Option<(FileStamp, String)>> {
        self.conn
            .prepare_cached("SELECT size, modified_ns, hash FROM file_stamps WHERE path = ?1")?
            .query_row([path], |row| {
                Ok((FileStamp { size: row.get(0)?, modified_ns: row.get(1)? }, row.get(2)?))
            })
            .optional()
    }

    /// Records the hash computed for each file, in one transaction.
    pub fn put_stamps(&self, stamps: &[(&str, FileStamp, &str)]) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO file_stamps (path, size, modified_ns, hash) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET
                     size = excluded.size, modified_ns = excluded.modified_ns, hash = excluded.hash",
            )?;
            for (path, stamp, hash) in stamps {
                insert.execute(rusqlite::params![path, stamp.size, stamp.modified_ns, hash])?;
            }
        }
        tx.commit()
    }

    /// Drafts not yet generated into a book, as the webview saved them. The
    /// JSON is the webview's own record; nothing here reads inside it.
    pub fn list_drafts(&self) -> rusqlite::Result<Vec<String>> {
        let mut rows = self.conn.prepare("SELECT json FROM drafts ORDER BY id")?;
        let drafts = rows.query_map([], |row| row.get(0))?.collect();
        drafts
    }

    pub fn save_draft(&self, id: i64, json: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO drafts (id, json) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = unixepoch()",
            rusqlite::params![id, json],
        )?;
        Ok(())
    }

    pub fn delete_draft(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM drafts WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Persists a new project. `book` is serialised whole into `book_json`
    /// (see `project.rs` for why); `page_count`/`photo_count` are
    /// denormalised alongside it so `list_projects` never has to parse
    /// JSON. Returns the new row's id.
    ///
    /// `photo_hashes` is the content hash of every photo the book was
    /// assembled against, in that slice's own order -- see
    /// `Project::photo_hashes` for why the ORDER and the FULL set are both
    /// load-bearing. Written in the same transaction as the project row, so
    /// a half-written project (a book whose photo list is missing or short)
    /// is not a state that can exist.
    ///
    /// `overrides` is written in that SAME transaction, for the same reason.
    /// The user's include/exclude decisions are the only record of what they
    /// asked for that differs from what the engine would have chosen; a
    /// project that reopened without them would silently revert every one of
    /// those decisions to `Auto`, which is exactly the "the analysis was
    /// lost" failure this table exists to avoid repeating.
    pub fn save_project(
        &self,
        name: &str,
        source_folder: &str,
        book: &Book,
        photo_hashes: &[String],
        overrides: &Overrides,
    ) -> rusqlite::Result<i64> {
        self.save_project_in(name, &[source_folder.to_string()], book, photo_hashes, overrides)
    }

    /// As `save_project`, for a book drawn from several folders. The first
    /// folder also lands in `projects.source_folder`, which the list reads
    /// for its label and which every row saved before `project_folders`
    /// existed relies on; the full list lives in `project_folders`, in the
    /// order the user picked them.
    pub fn save_project_in(
        &self,
        name: &str,
        source_folders: &[String],
        book: &Book,
        photo_hashes: &[String],
        overrides: &Overrides,
    ) -> rusqlite::Result<i64> {
        let book_json = serde_json::to_string(book)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (page_count, photo_count) = project::book_counts(book);
        let first = source_folders.first().cloned().unwrap_or_default();

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO projects (name, source_folder, page_count, photo_count, book_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![name, first, page_count, photo_count, book_json],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO project_folders (project_id, position, path) VALUES (?1, ?2, ?3)",
            )?;
            for (position, path) in source_folders.iter().enumerate() {
                stmt.execute(rusqlite::params![id, position as i64, path])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO project_photos (project_id, position, hash) VALUES (?1, ?2, ?3)",
            )?;
            for (position, hash) in photo_hashes.iter().enumerate() {
                stmt.execute(rusqlite::params![id, position as i64, hash])?;
            }
        }
        {
            let mut stmt = tx.prepare(
                "INSERT INTO project_photo_overrides (project_id, hash, state) VALUES (?1, ?2, ?3)",
            )?;
            for (hash, state) in overrides.iter() {
                stmt.execute(rusqlite::params![id, hash, state.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Loads a project and its export history. `None` if `id` does not
    /// exist.
    pub fn load_project(&self, id: i64) -> rusqlite::Result<Option<Project>> {
        let row = self
            .conn
            .query_row(
                "SELECT name, source_folder, book_json, created_at, updated_at
                 FROM projects WHERE id = ?1 AND deleted_at IS NULL",
                rusqlite::params![id],
                |row| {
                    let name: String = row.get(0)?;
                    let source_folder: String = row.get(1)?;
                    let book_json: String = row.get(2)?;
                    let created_at: i64 = row.get(3)?;
                    let updated_at: i64 = row.get(4)?;
                    Ok((name, source_folder, book_json, created_at, updated_at))
                },
            )
            .optional()?;

        let Some((name, source_folder, book_json, created_at, updated_at)) = row else {
            return Ok(None);
        };

        let book: Book = serde_json::from_str(&book_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;

        // `ORDER BY position` is not cosmetic: `Placement::photo_index`
        // indexes this list positionally, so any other order silently
        // repoints every placement at a different photo.
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM project_photos WHERE project_id = ?1 ORDER BY position ASC")?;
        let photo_hashes = stmt
            .query_map(rusqlite::params![id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut stmt = self
            .conn
            .prepare("SELECT hash, state FROM project_photo_overrides WHERE project_id = ?1")?;
        let overrides = stmt
            .query_map(rusqlite::params![id], |row| {
                let hash: String = row.get(0)?;
                let token: String = row.get(1)?;
                // An unrecognised token fails the LOAD rather than degrading
                // to `Auto`. Silently forgetting a decision the user made is
                // the failure this feature exists to prevent, and it would be
                // invisible: the project would simply open with the engine's
                // own verdict and look correct.
                let state = Override::from_token(&token).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        format!("unknown photo override state {token:?}").into(),
                    )
                })?;
                Ok((hash, state))
            })?
            .collect::<rusqlite::Result<Overrides>>()?;

        let source_folders = self.project_folders(id, &source_folder)?;

        let mut stmt = self.conn.prepare(
            "SELECT output_dir, format, file_count, exported_at
             FROM project_exports WHERE project_id = ?1 ORDER BY exported_at ASC, id ASC",
        )?;
        let exports = stmt
            .query_map(rusqlite::params![id], |row| {
                let output_dir: String = row.get(0)?;
                let format: String = row.get(1)?;
                let file_count: i64 = row.get(2)?;
                let at: i64 = row.get(3)?;
                Ok(ExportRecord { at, output_dir, format, file_count: file_count as usize })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Some(Project {
            id,
            name,
            source_folder,
            source_folders,
            created_at,
            updated_at,
            book,
            photo_hashes,
            overrides,
            exports,
        }))
    }

    /// The folders a project was analysed from, in the order they were
    /// picked. A row saved before `project_folders` existed has none, and its
    /// one `source_folder` column IS the list.
    fn project_folders(&self, id: i64, source_folder: &str) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM project_folders WHERE project_id = ?1 ORDER BY position ASC")?;
        let folders = stmt
            .query_map(rusqlite::params![id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(if folders.is_empty() { vec![source_folder.to_string()] } else { folders })
    }

    /// Summaries for every saved project, newest-updated first.
    pub fn list_projects(&self) -> rusqlite::Result<Vec<ProjectSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, source_folder, page_count, photo_count, created_at, updated_at, favourite
             FROM projects WHERE deleted_at IS NULL ORDER BY updated_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ProjectSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    source_folder: row.get(2)?,
                    source_folders: Vec::new(),
                    page_count: row.get(3)?,
                    photo_count: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    favourite: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|mut summary| {
                summary.source_folders = self.project_folders(summary.id, &summary.source_folder)?;
                Ok(summary)
            })
            .collect()
    }

    /// Appends an export to a project's history. The persisted timestamp
    /// always comes from SQLite's `unixepoch()`, not `rec.at` -- see the
    /// doc comment on `ExportRecord`.
    pub fn record_export(&self, project_id: i64, rec: &ExportRecord) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO project_exports (project_id, output_dir, format, file_count)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![project_id, rec.output_dir, rec.format, rec.file_count as i64],
        )?;
        Ok(())
    }

    /// Replaces a project's book after an edit, re-deriving the denormalised
    /// counts so the list stays truthful. Returns the rows affected (0 or 1),
    /// like `rename_project`. The photo list and overrides are untouched: an
    /// edit rearranges the book over the same photo slice, which is exactly
    /// what keeps `Placement::photo_index` valid.
    pub fn update_project_book(&self, id: i64, book: &Book) -> rusqlite::Result<usize> {
        let book_json = serde_json::to_string(book)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let (page_count, photo_count) = project::book_counts(book);
        self.conn.execute(
            "UPDATE projects SET book_json = ?1, page_count = ?2, photo_count = ?3,
             updated_at = unixepoch() WHERE id = ?4",
            rusqlite::params![book_json, page_count, photo_count, id],
        )
    }

    /// Renames a project. Returns the number of rows affected (0 or 1), so a
    /// caller can tell "renamed" from "that id does not exist" without a
    /// separate lookup -- `rename_project` (the Tauri command) turns 0 into
    /// the same "project no longer exists" error `open_project` already
    /// gives for a missing `load_project`.
    ///
    /// `updated_at` is stamped again here, same as `record_export`: a rename
    /// is a real edit, and `list_projects`' "newest first" ordering should
    /// reflect it.
    pub fn rename_project(&self, id: i64, name: &str) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE projects SET name = ?1, updated_at = unixepoch() WHERE id = ?2",
            rusqlite::params![name, id],
        )
    }

    /// Stars or unstars a project. Returns the rows affected, 0 for an id
    /// that is gone or in the trash. `updated_at` is left alone, unlike
    /// `rename_project`: starring files a book, it does not edit it.
    pub fn set_favourite(&self, id: i64, favourite: bool) -> rusqlite::Result<usize> {
        self.conn.execute(
            "UPDATE projects SET favourite = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            rusqlite::params![favourite, id],
        )
    }

    /// Moves a project to the trash: it is no longer listed or loadable, but
    /// nothing is removed, so `restore_project` can undo it until
    /// `purge_deleted` clears it.
    pub fn delete_project(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE projects SET deleted_at = unixepoch() WHERE id = ?1 AND deleted_at IS NULL",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    /// Takes a project back out of the trash. Returns the rows affected, 0
    /// once it has been purged.
    pub fn restore_project(&self, id: i64) -> rusqlite::Result<usize> {
        self.conn.execute("UPDATE projects SET deleted_at = NULL WHERE id = ?1", rusqlite::params![id])
    }

    /// Permanently removes every project trashed at or before `before`
    /// (unix seconds). Returns how many.
    pub fn purge_deleted(&self, before: i64) -> rusqlite::Result<usize> {
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM projects WHERE deleted_at <= ?1")?
            .query_map(rusqlite::params![before], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for &id in &ids {
            self.purge_project(id)?;
        }
        Ok(ids.len())
    }

    /// Removes a project for good, with every export row recorded against
    /// it, its photo list, its folder list and the user's overrides for it.
    /// The child rows go first: `migrate()` turns `PRAGMA foreign_keys` on,
    /// and no child table declares `ON DELETE CASCADE`.
    pub(crate) fn purge_project(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM project_exports WHERE project_id = ?1", rusqlite::params![id])?;
        self.conn.execute("DELETE FROM project_photos WHERE project_id = ?1", rusqlite::params![id])?;
        self.conn.execute(
            "DELETE FROM project_photo_overrides WHERE project_id = ?1",
            rusqlite::params![id],
        )?;
        self.conn.execute("DELETE FROM project_folders WHERE project_id = ?1", rusqlite::params![id])?;
        self.conn.execute("DELETE FROM projects WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_deleted_project_is_neither_listed_nor_loadable() {
        let db = Db::open_in_memory().unwrap();
        let keep = db.save_project("Keep", "/tmp/keep", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let gone = db.save_project("Gone", "/tmp/gone", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        db.delete_project(gone).unwrap();

        let ids: Vec<i64> = db.list_projects().unwrap().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![keep]);
        assert!(db.load_project(gone).unwrap().is_none());
    }

    #[test]
    fn a_restored_project_comes_back_whole() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hz22", Override::Include);
        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &overrides).unwrap();
        let before = db.load_project(id).unwrap().unwrap();

        db.delete_project(id).unwrap();
        assert_eq!(db.restore_project(id).unwrap(), 1);

        let after = db.load_project(id).unwrap().expect("restored project loads");
        assert_eq!(after.photo_hashes, before.photo_hashes);
        assert_eq!(after.overrides, before.overrides);
        assert_eq!(db.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn restoring_a_purged_project_restores_nothing() {
        let db = Db::open_in_memory().unwrap();
        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        db.delete_project(id).unwrap();
        db.purge_project(id).unwrap();

        assert_eq!(db.restore_project(id).unwrap(), 0);
    }

    #[test]
    fn purging_removes_only_projects_deleted_before_the_cutoff() {
        let db = Db::open_in_memory().unwrap();
        let save = |name: &str| {
            db.save_project(name, "/tmp/x", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap()
        };
        let old = save("Deleted long ago");
        let recent = save("Deleted just now");
        let live = save("Never deleted");
        db.delete_project(old).unwrap();
        db.delete_project(recent).unwrap();
        db.conn.execute("UPDATE projects SET deleted_at = 100 WHERE id = ?1", rusqlite::params![old]).unwrap();
        db.conn.execute("UPDATE projects SET deleted_at = 300 WHERE id = ?1", rusqlite::params![recent]).unwrap();
        db.conn.execute("UPDATE projects SET updated_at = 0, created_at = 0 WHERE id = ?1", rusqlite::params![live]).unwrap();

        assert_eq!(db.purge_deleted(200).unwrap(), 1);

        let left: Vec<i64> = db
            .conn
            .prepare("SELECT id FROM projects ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(left, vec![recent, live]);
        assert_eq!(db.restore_project(recent).unwrap(), 1, "a recent delete can still be undone");
    }

    #[test]
    fn migrate_adds_deleted_at_to_a_database_that_predates_it() {
        let dir = std::env::temp_dir().join(format!("pbg-deleted-at-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE projects (
                     id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                     source_folder TEXT NOT NULL, page_count INTEGER NOT NULL,
                     photo_count INTEGER NOT NULL, book_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch()));
                 INSERT INTO projects (name, source_folder, page_count, photo_count, book_json)
                     VALUES ('Old', '/tmp/old', 2, 4, '{}');",
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();
        db.migrate().unwrap();

        assert_eq!(db.list_projects().unwrap().len(), 1, "an existing book is not deleted");
        db.delete_project(1).unwrap();
        assert!(db.list_projects().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_adds_favourite_to_a_database_that_predates_it() {
        let dir = std::env::temp_dir().join(format!("pbg-favourite-migrate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.sqlite");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE projects (
                     id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                     source_folder TEXT NOT NULL, page_count INTEGER NOT NULL,
                     photo_count INTEGER NOT NULL, book_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                     deleted_at INTEGER);
                 INSERT INTO projects (name, source_folder, page_count, photo_count, book_json)
                     VALUES ('Old', '/tmp/old', 2, 4, '{}');",
            )
            .unwrap();
        }

        let db = Db::open(&path).unwrap();
        db.migrate().unwrap();

        assert!(!db.list_projects().unwrap()[0].favourite, "an existing book starts unfavourited");
        db.set_favourite(1, true).unwrap();
        assert!(db.list_projects().unwrap()[0].favourite);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_drafts_come_back_in_id_order() {
        let db = Db::open_in_memory().unwrap();
        db.save_draft(2, r#"{"name":"Bali"}"#).unwrap();
        db.save_draft(1, r#"{"name":"Kyoto"}"#).unwrap();
        assert_eq!(db.list_drafts().unwrap(), vec![r#"{"name":"Kyoto"}"#, r#"{"name":"Bali"}"#]);
    }

    #[test]
    fn saving_a_draft_again_replaces_it() {
        let db = Db::open_in_memory().unwrap();
        db.save_draft(1, r#"{"name":"Kyoto"}"#).unwrap();
        db.save_draft(1, r#"{"name":"Kyoto 2026"}"#).unwrap();
        assert_eq!(db.list_drafts().unwrap(), vec![r#"{"name":"Kyoto 2026"}"#]);
    }

    #[test]
    fn a_deleted_draft_is_gone_and_the_others_stay() {
        let db = Db::open_in_memory().unwrap();
        db.save_draft(1, r#"{"name":"Kyoto"}"#).unwrap();
        db.save_draft(2, r#"{"name":"Bali"}"#).unwrap();
        db.delete_draft(1).unwrap();
        assert_eq!(db.list_drafts().unwrap(), vec![r#"{"name":"Bali"}"#]);
    }

    use super::*;
    use crate::book::cull::Override;
    use crate::book::pace::{Book, Page, Placement};
    use crate::geometry::{Rect, Side};

    /// Seven hashes, deliberately NOT in sorted order and NOT equal to their
    /// own positions: `Placement::photo_index` indexes this list
    /// positionally, so a fixture whose order happens to match any incidental
    /// ordering (alphabetical, insertion-by-id) could not tell a correct
    /// round trip from one that re-sorted on the way out. Seven because
    /// `fixture_book` places 4 photos and reports 3 dropped.
    fn fixture_hashes() -> Vec<String> {
        ["hf00", "ha11", "hz22", "hb33", "hy44", "hc55", "hx66"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Two pages, two placements per page, distinct non-round `z` values,
    /// and `slot_rect`/`crop` rects that differ from each other AND carry
    /// enough decimal precision to catch rounding. A one-page,
    /// one-placement, round-numbers fixture cannot detect most of the
    /// round-trip mutations this file pins.
    fn fixture_book() -> Book {
        Book {
            controls: Default::default(),
            seed: 424_242,
            dropped: 3,
            pages: vec![
                Page {
                    number: 1,
                    side: Side::Right,
                    template_id: "spread-a:right".into(),
                    placements: vec![
                        Placement {
                            photo_index: 0,
                            slot_rect: Rect::new(0.0123, 0.0456, 0.4321, 0.2109),
                            crop: Rect::new(0.1111, 0.2222, 0.3333, 0.4444),
                            z: 1,
                        },
                        Placement {
                            photo_index: 1,
                            slot_rect: Rect::new(0.5555, 0.1234, 0.4321, 0.2109),
                            crop: Rect::new(0.0987, 0.0654, 0.3210, 0.1987),
                            z: 2,
                        },
                    ],
                },
                Page {
                    number: 2,
                    side: Side::Left,
                    template_id: "spread-b:left".into(),
                    placements: vec![
                        Placement {
                            photo_index: 2,
                            slot_rect: Rect::new(0.0789, 0.0912, 0.4567, 0.2345),
                            crop: Rect::new(0.2468, 0.1357, 0.3691, 0.2580),
                            z: 3,
                        },
                        Placement {
                            photo_index: 3,
                            slot_rect: Rect::new(0.6013, 0.0456, 0.3579, 0.2864),
                            crop: Rect::new(0.0135, 0.0246, 0.4680, 0.3579),
                            z: 4,
                        },
                    ],
                },
            ],
        }
    }

    /// A book drawn from several folders remembers all of them, in the order
    /// they were picked; a row saved before `project_folders` existed still
    /// reports its one folder as the list.
    #[test]
    fn a_project_remembers_every_folder_it_was_analysed_from_in_order() {
        let db = Db::open_in_memory().unwrap();
        let folders = vec!["/Users/j/Photos/kyoto".to_string(), "/Users/j/Phone/2026-05".to_string()];
        let id = db
            .save_project_in("Kyoto", &folders, &fixture_book(), &["h0".into()], &Overrides::new())
            .unwrap();

        let loaded = db.load_project(id).unwrap().unwrap();
        assert_eq!(loaded.source_folders, folders);
        assert_eq!(loaded.source_folder, "/Users/j/Photos/kyoto", "the first folder is the label");
        let listed = db.list_projects().unwrap();
        assert_eq!(listed[0].source_folders, folders);

        // A row from before the folder table: delete its folder rows to
        // stand in for a database that never had them.
        db.conn.execute("DELETE FROM project_folders WHERE project_id = ?1", rusqlite::params![id]).unwrap();
        let legacy = db.load_project(id).unwrap().unwrap();
        assert_eq!(legacy.source_folders, vec!["/Users/j/Photos/kyoto".to_string()]);

        db.purge_project(id).unwrap();
        let left: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM project_folders WHERE project_id = ?1", rusqlite::params![id], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn migration_is_additive_and_preserves_the_features_cache() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"v":1}"#).unwrap();

        // Re-running the migration against a live connection must be a
        // no-op for existing data -- `CREATE TABLE IF NOT EXISTS` is what
        // makes this safe, and this test is what proves it stays safe.
        db.migrate().unwrap();

        assert_eq!(db.get_features("abc").unwrap().unwrap(), r#"{"v":1}"#);
    }

    #[test]
    fn migrate_turns_foreign_key_enforcement_on_for_the_connection() {
        let db = Db::open_in_memory().unwrap();

        // A `PRAGMA foreign_keys` statement is a silent no-op if issued
        // inside a transaction, so this reads the pragma back rather than
        // just trusting that the statement was written into migrate().
        let fk: i64 = db.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();

        assert_eq!(fk, 1, "foreign_keys must read ON after migrate() has run");
    }

    #[test]
    fn foreign_keys_reject_an_export_row_for_a_project_that_does_not_exist() {
        let db = Db::open_in_memory().unwrap();

        let result = db.conn.execute(
            "INSERT INTO project_exports (project_id, output_dir, format, file_count)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![999_i64, "/tmp/orphan", "jpeg", 1_i64],
        );

        assert!(
            result.is_err(),
            "inserting an export row against a non-existent project_id must be rejected, got {result:?}"
        );
    }

    #[test]
    fn round_trips_a_saved_project_through_load() {
        let db = Db::open_in_memory().unwrap();
        let book = fixture_book();

        let id = db.save_project("Kyoto Trip", "/Users/j/Photos/kyoto", &book, &fixture_hashes(), &Overrides::new()).unwrap();
        let loaded = db.load_project(id).unwrap().expect("project should exist");

        assert_eq!(loaded.book, book);
        assert_eq!(loaded.name, "Kyoto Trip");
        assert_eq!(loaded.source_folder, "/Users/j/Photos/kyoto");
        assert!(loaded.exports.is_empty());
    }

    #[test]
    fn load_project_returns_none_for_unknown_id() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.load_project(999).unwrap().is_none());
    }

    /// Spec 5.4a: a project stores the photo set the book is defined over,
    /// by content hash. The ORDER is the contract -- `Placement::photo_index`
    /// indexes this list positionally -- so this asserts the exact sequence,
    /// not a set.
    #[test]
    fn round_trips_the_photo_hash_list_in_its_original_order() {
        let db = Db::open_in_memory().unwrap();
        let hashes = fixture_hashes();

        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &hashes, &Overrides::new()).unwrap();
        let loaded = db.load_project(id).unwrap().unwrap();

        assert_eq!(loaded.photo_hashes, hashes);
        // Sanity: the fixture is not already sorted, so a query that lost its
        // `ORDER BY position` (or re-sorted by hash) is distinguishable from
        // one that kept insertion order.
        let mut sorted = hashes.clone();
        sorted.sort();
        assert_ne!(sorted, hashes, "fixture must not be pre-sorted");
    }

    /// Two byte-identical files in one folder hash the same and are two real
    /// placements. The list is positional, not a set, so it must keep both.
    #[test]
    fn keeps_a_repeated_hash_at_both_of_its_positions() {
        let db = Db::open_in_memory().unwrap();
        let hashes: Vec<String> =
            ["hdup", "hb11", "hdup"].iter().map(|s| s.to_string()).collect();

        let id = db.save_project("Dupes", "/tmp/dupes", &fixture_book(), &hashes, &Overrides::new()).unwrap();

        assert_eq!(db.load_project(id).unwrap().unwrap().photo_hashes, hashes);
    }

    /// A project saved before `project_photos` existed has no rows there.
    /// That must read back as an empty list, not fail the load -- the caller
    /// turns it into "re-analyse this folder", and a project the user cannot
    /// even list is worse than one they cannot export.
    #[test]
    fn a_project_with_no_stored_photo_list_loads_with_an_empty_one() {
        let db = Db::open_in_memory().unwrap();
        let id = db.save_project("Legacy", "/tmp/legacy", &fixture_book(), &[], &Overrides::new()).unwrap();

        let loaded = db.load_project(id).unwrap().expect("the project must still load");

        assert!(loaded.photo_hashes.is_empty());
        assert_eq!(loaded.book, fixture_book(), "the book itself is unaffected");
    }

    /// `save_project` writes the project row and its photo list in one
    /// transaction, so a failure partway through must leave NO project row
    /// at all rather than one whose photo list is missing or short -- a
    /// project that looks saved but cannot be exported is worse than a save
    /// that visibly failed.
    ///
    /// The failure is forced by pre-claiming the `(project_id, position)`
    /// primary key the next save will need. Foreign keys are switched off
    /// only for the duration of that squatting insert, since the project row
    /// it references does not exist yet by construction.
    #[test]
    fn a_failed_photo_list_write_leaves_no_half_written_project() {
        let db = Db::open_in_memory().unwrap();
        let existing =
            db.save_project("First", "/tmp/first", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let next_id = existing + 1;

        db.conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        db.conn
            .execute(
                "INSERT INTO project_photos (project_id, position, hash) VALUES (?1, 0, 'squatter')",
                rusqlite::params![next_id],
            )
            .unwrap();
        db.conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();

        let before: i64 =
            db.conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();

        let result =
            db.save_project("Second", "/tmp/second", &fixture_book(), &fixture_hashes(), &Overrides::new());

        assert!(result.is_err(), "the colliding photo-list insert must fail the save");
        let after: i64 =
            db.conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0)).unwrap();
        assert_eq!(after, before, "a rolled-back save must leave no project row behind");
        assert!(
            db.load_project(next_id).unwrap().is_none(),
            "and the id it would have taken must still be unused"
        );
    }

    /// **Overrides survive a save and a reopen.**
    ///
    /// They are the only record of what the user asked for that the engine
    /// would not have chosen by itself, and nothing about the saved `Book`
    /// encodes WHY a photo is in it -- so a project that reopened without
    /// them would silently revert every decision to `Auto` and look correct
    /// while doing it.
    ///
    /// Both states are asserted, and a hash with NO decision is asserted to
    /// come back with none: a round trip that stored only one of the two
    /// states, or that turned every listed hash into the same state, passes a
    /// single-state fixture.
    #[test]
    fn round_trips_the_users_include_and_exclude_decisions() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hz22", Override::Include);
        overrides.set("hb33", Override::Exclude);
        overrides.set("hf00", Override::Include);

        let id = db
            .save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &overrides)
            .unwrap();
        let loaded = db.load_project(id).unwrap().unwrap();

        assert_eq!(loaded.overrides, overrides);
        assert_eq!(loaded.overrides.get("hz22"), Override::Include);
        assert_eq!(loaded.overrides.get("hb33"), Override::Exclude);
        assert_eq!(loaded.overrides.get("ha11"), Override::Auto, "an untouched photo stays auto");
        assert_eq!(loaded.overrides.len(), 3, "and nothing else was invented");
    }

    /// A project saved with no decisions at all reads back as none, not as a
    /// failure and not as a map of `Auto` entries.
    #[test]
    fn a_project_with_no_overrides_loads_with_an_empty_map() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .save_project("Plain", "/tmp/plain", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();

        assert!(db.load_project(id).unwrap().unwrap().overrides.is_empty());
    }

    /// An override row whose state this build does not recognise fails the
    /// load rather than degrading to `Auto`. Silently forgetting a decision
    /// is invisible -- the project simply opens with the engine's verdict and
    /// looks right -- which makes it worse than refusing to open at all.
    #[test]
    fn an_unrecognised_override_state_fails_the_load_rather_than_reverting_to_auto() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO project_photo_overrides (project_id, hash, state) VALUES (?1, 'hz22', 'pin')",
                rusqlite::params![id],
            )
            .unwrap();

        let result = db.load_project(id);

        assert!(result.is_err(), "an unknown state must not load as auto, got {result:?}");
    }

    /// The overrides go in the SAME transaction as the project row: a save
    /// that half-succeeded would leave a book whose photo selection is
    /// somebody else's.
    #[test]
    fn a_failed_override_write_leaves_no_half_written_project() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hz22", Override::Include);
        let existing = db
            .save_project("First", "/tmp/first", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();
        let next_id = existing + 1;

        db.conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        db.conn
            .execute(
                "INSERT INTO project_photo_overrides (project_id, hash, state) VALUES (?1, 'hz22', 'include')",
                rusqlite::params![next_id],
            )
            .unwrap();
        db.conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();

        let result =
            db.save_project("Second", "/tmp/second", &fixture_book(), &fixture_hashes(), &overrides);

        assert!(result.is_err(), "the colliding override insert must fail the save");
        assert!(db.load_project(next_id).unwrap().is_none(), "and roll the project row back");
    }

    #[test]
    fn purge_project_removes_the_users_overrides_too() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hz22", Override::Include);
        let keep = db
            .save_project("Keep", "/tmp/keep", &fixture_book(), &fixture_hashes(), &overrides)
            .unwrap();
        let gone = db
            .save_project("Gone", "/tmp/gone", &fixture_book(), &fixture_hashes(), &overrides)
            .unwrap();
        let rows = |id: i64| -> i64 {
            db.conn
                .query_row(
                    "SELECT COUNT(*) FROM project_photo_overrides WHERE project_id = ?1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(rows(gone), 1, "sanity: the overrides were written");

        db.purge_project(gone).unwrap();

        assert_eq!(rows(gone), 0);
        assert_eq!(rows(keep), 1, "and only that project's");
    }

    #[test]
    fn purge_project_removes_its_photo_list_too() {
        let db = Db::open_in_memory().unwrap();
        let keep =
            db.save_project("Keep", "/tmp/keep", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let gone =
            db.save_project("Gone", "/tmp/gone", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        let photo_rows = |id: i64| -> i64 {
            db.conn
                .query_row(
                    "SELECT COUNT(*) FROM project_photos WHERE project_id = ?1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(photo_rows(gone), 7, "sanity: the photo list was written");

        db.purge_project(gone).unwrap();

        assert_eq!(photo_rows(gone), 0, "the deleted project's photo list must go with it");
        assert_eq!(photo_rows(keep), 7, "and only that project's");
    }

    #[test]
    fn save_project_denormalises_page_and_photo_counts_for_listing() {
        let db = Db::open_in_memory().unwrap();
        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        let summaries = db.list_projects().unwrap();
        let summary = summaries.iter().find(|s| s.id == id).unwrap();

        assert_eq!(summary.page_count, 2);
        assert_eq!(summary.photo_count, 4);
    }

    #[test]
    fn list_projects_orders_newest_updated_first() {
        let db = Db::open_in_memory().unwrap();
        let older = db.save_project("Older", "/tmp/older", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let newer = db.save_project("Newer", "/tmp/newer", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        // `unixepoch()` has one-second granularity, so two saves in the
        // same test can easily tie -- force a deterministic ordering
        // directly rather than depending on wall-clock timing.
        db.conn
            .execute("UPDATE projects SET updated_at = 100 WHERE id = ?1", rusqlite::params![older])
            .unwrap();
        db.conn
            .execute("UPDATE projects SET updated_at = 200 WHERE id = ?1", rusqlite::params![newer])
            .unwrap();

        let ids: Vec<i64> = db.list_projects().unwrap().iter().map(|s| s.id).collect();
        assert_eq!(ids, vec![newer, older]);
    }

    /// Favouriting files a book, it does not edit it: the list's "newest
    /// first" order must not move a book just because it was starred.
    #[test]
    fn set_favourite_persists_without_touching_updated_at() {
        let db = Db::open_in_memory().unwrap();
        let kyoto = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let bali = db.save_project("Bali", "/tmp/bali", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        db.conn.execute("UPDATE projects SET updated_at = 100", []).unwrap();

        assert_eq!(db.set_favourite(kyoto, true).unwrap(), 1);

        let listed = db.list_projects().unwrap();
        let find = |id: i64| listed.iter().find(|s| s.id == id).unwrap();
        assert!(find(kyoto).favourite);
        assert!(!find(bali).favourite, "only the named book is starred");
        assert_eq!(find(kyoto).updated_at, 100);

        db.set_favourite(kyoto, false).unwrap();
        assert!(!db.list_projects().unwrap().iter().any(|s| s.favourite));
    }

    #[test]
    fn set_favourite_of_an_unknown_or_deleted_id_affects_no_rows() {
        let db = Db::open_in_memory().unwrap();
        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        db.delete_project(id).unwrap();
        assert_eq!(db.set_favourite(id, true).unwrap(), 0);
        assert_eq!(db.set_favourite(9999, true).unwrap(), 0);
    }

    #[test]
    fn record_export_appends_and_load_project_returns_them_in_order() {
        let db = Db::open_in_memory().unwrap();
        let id = db.save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        db.record_export(
            id,
            &ExportRecord { at: 0, output_dir: "/tmp/out1".into(), format: "jpeg".into(), file_count: 12 },
        )
        .unwrap();
        db.record_export(
            id,
            &ExportRecord { at: 0, output_dir: "/tmp/out2".into(), format: "png".into(), file_count: 7 },
        )
        .unwrap();

        let project = db.load_project(id).unwrap().unwrap();
        assert_eq!(project.exports.len(), 2);
        assert_eq!(project.exports[0].output_dir, "/tmp/out1");
        assert_eq!(project.exports[0].format, "jpeg");
        assert_eq!(project.exports[0].file_count, 12);
        assert_eq!(project.exports[1].output_dir, "/tmp/out2");
        assert_eq!(project.exports[1].file_count, 7);
    }

    #[test]
    fn purge_project_removes_the_project_and_its_export_rows_only() {
        let db = Db::open_in_memory().unwrap();
        let keep = db.save_project("Keep", "/tmp/keep", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let gone = db.save_project("Gone", "/tmp/gone", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        db.record_export(
            gone,
            &ExportRecord { at: 0, output_dir: "/tmp/gone-out".into(), format: "jpeg".into(), file_count: 3 },
        )
        .unwrap();
        db.record_export(
            keep,
            &ExportRecord { at: 0, output_dir: "/tmp/keep-out".into(), format: "jpeg".into(), file_count: 5 },
        )
        .unwrap();

        let count = |table: &str| -> i64 {
            db.conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
        };
        assert_eq!(count("projects"), 2, "sanity: both projects present before delete");
        assert_eq!(count("project_exports"), 2, "sanity: both export rows present before delete");

        db.purge_project(gone).unwrap();

        assert_eq!(count("projects"), 1, "only the deleted project should be gone");
        assert_eq!(count("project_exports"), 1, "only the deleted project's export row should be gone");
        assert!(db.load_project(gone).unwrap().is_none());
        assert!(db.load_project(keep).unwrap().is_some());
    }

    /// Renaming is the only edit a saved project gets after generation: the
    /// name is set once at generate time from the source folder, and this is
    /// how the user fixes it later. `updated_at` moves too, on purpose -- a
    /// rename is a real edit, and `list_projects`' "newest first" ordering
    /// should reflect it, exactly as it would for a re-export.
    #[test]
    fn rename_project_changes_the_name_and_bumps_updated_at() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .save_project("Kyoto Trip", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();
        db.conn
            .execute("UPDATE projects SET updated_at = 100 WHERE id = ?1", rusqlite::params![id])
            .unwrap();

        let affected = db.rename_project(id, "Kyoto Trip (final)").unwrap();

        assert_eq!(affected, 1);
        let loaded = db.load_project(id).unwrap().unwrap();
        assert_eq!(loaded.name, "Kyoto Trip (final)");
        assert!(loaded.updated_at > 100, "rename must bump updated_at, got {}", loaded.updated_at);
    }

    /// A rename touches ONLY the targeted row -- the book, the photo list,
    /// the overrides and every other project's name must survive untouched.
    #[test]
    fn rename_project_touches_only_the_named_project() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = Overrides::new();
        overrides.set("hz22", Override::Include);
        let target = db
            .save_project("Kyoto Trip", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &overrides)
            .unwrap();
        let other =
            db.save_project("Osaka Trip", "/tmp/osaka", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        db.rename_project(target, "Kyoto Trip (final)").unwrap();

        let target_loaded = db.load_project(target).unwrap().unwrap();
        assert_eq!(target_loaded.book, fixture_book(), "the book itself is unaffected by a rename");
        assert_eq!(target_loaded.overrides, overrides, "and neither are the user's decisions");
        let other_loaded = db.load_project(other).unwrap().unwrap();
        assert_eq!(other_loaded.name, "Osaka Trip", "a rename must not leak to a different project");
    }

    /// Renaming an id that does not exist affects no rows rather than
    /// erroring -- the caller (the `rename_project` command) is the one that
    /// turns "zero rows" into "project no longer exists", exactly as
    /// `open_project` already does for a missing `load_project`.
    #[test]
    fn rename_project_of_an_unknown_id_affects_no_rows() {
        let db = Db::open_in_memory().unwrap();
        let affected = db.rename_project(999, "Anything").unwrap();
        assert_eq!(affected, 0);
    }

    /// **Duplicate names are allowed, on purpose.** `projects.name` has no
    /// `UNIQUE` constraint -- `save_project` never required distinct names
    /// (two folders named "Kyoto" on different disks are a real scenario, and
    /// inventing a collision rule for rename alone, when generate never had
    /// one, would be a new restriction nobody asked for). Asserts both rows
    /// really do carry the same name afterwards -- not just "no error", but
    /// the actual state both callers would then read back.
    #[test]
    fn rename_project_allows_two_projects_to_share_a_name() {
        let db = Db::open_in_memory().unwrap();
        let first =
            db.save_project("Kyoto Trip", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();
        let second =
            db.save_project("Osaka Trip", "/tmp/osaka", &fixture_book(), &fixture_hashes(), &Overrides::new()).unwrap();

        let affected = db.rename_project(second, "Kyoto Trip").unwrap();

        assert_eq!(affected, 1, "the rename itself must succeed, not be rejected as a collision");
        assert_eq!(db.load_project(first).unwrap().unwrap().name, "Kyoto Trip");
        assert_eq!(
            db.load_project(second).unwrap().unwrap().name,
            "Kyoto Trip",
            "both rows now legitimately share the name"
        );
    }

    /// **The analysis cache must survive a delete.** `features` is keyed by
    /// content hash and shared across every project -- it is the expensive
    /// thing, the result of running Apple Vision over every photo. A project
    /// delete that reached into it would force a full re-analysis the next
    /// time the same folder (or an overlapping one) is opened, which is
    /// exactly the cost the user objected to.
    #[test]
    fn purge_project_leaves_the_features_cache_untouched() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("hz22", "/tmp/kyoto/a.jpg", r#"{"v":1}"#).unwrap();
        db.put_features("hb33", "/tmp/kyoto/b.jpg", r#"{"v":2}"#).unwrap();
        let id = db
            .save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();

        db.purge_project(id).unwrap();

        assert_eq!(
            db.get_features("hz22").unwrap(),
            Some(r#"{"v":1}"#.to_string()),
            "a project delete must never remove rows from the shared features cache"
        );
        assert_eq!(db.get_features("hb33").unwrap(), Some(r#"{"v":2}"#.to_string()));
    }

    /// **A project delete must never touch the filesystem.** Exported files
    /// live in a folder the user chose, and may already be uploaded to a
    /// printer -- removing a book from the app's own database must not reach
    /// onto their disk. `Db::delete_project` only issues `DELETE` statements
    /// against its own tables, so this proves that by observing the real
    /// world, not by reading the implementation.
    #[test]
    fn purge_project_does_not_touch_files_on_disk() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir()
            .join(format!("pbg-delete-project-fs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exported_file = dir.join("page-01.jpg");
        std::fs::write(&exported_file, b"fake exported jpeg bytes").unwrap();

        let id = db
            .save_project("Kyoto", "/tmp/kyoto", &fixture_book(), &fixture_hashes(), &Overrides::new())
            .unwrap();
        db.record_export(
            id,
            &ExportRecord {
                at: 0,
                output_dir: dir.to_string_lossy().into_owned(),
                format: "jpeg".into(),
                file_count: 1,
            },
        )
        .unwrap();

        db.purge_project(id).unwrap();

        assert!(
            exported_file.exists(),
            "delete_project must never remove files the user exported to disk"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn returns_none_for_unknown_hash() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.get_features("deadbeef").unwrap().is_none());
    }

    #[test]
    fn round_trips_features() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"width":100}"#).unwrap();
        assert_eq!(db.get_features("abc").unwrap().unwrap(), r#"{"width":100}"#);
    }

    #[test]
    fn overwrites_on_reanalysis_of_same_hash() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"v":1}"#).unwrap();
        db.put_features("abc", "/tmp/a.jpg", r#"{"v":2}"#).unwrap();
        assert_eq!(db.get_features("abc").unwrap().unwrap(), r#"{"v":2}"#);
    }

    #[test]
    fn ignores_rows_from_an_older_analyzer_version() {
        let db = Db::open_in_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO features (hash, path, json, analyzer_version) VALUES (?1,?2,?3,?4)",
                rusqlite::params!["old", "/tmp/o.jpg", "{}", ANALYZER_VERSION - 1],
            )
            .unwrap();
        assert!(db.get_features("old").unwrap().is_none());
    }

}
