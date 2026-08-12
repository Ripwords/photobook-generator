use rusqlite::{Connection, OptionalExtension};
use std::path::Path;

/// Bump this whenever the Swift analyzer's output changes shape or semantics.
/// Cached rows at an older version are ignored and re-analysed.
pub const ANALYZER_VERSION: u32 = 1;

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

    pub fn analyzer_version() -> u32 {
        ANALYZER_VERSION
    }

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS features (
                 hash             TEXT PRIMARY KEY,
                 path             TEXT NOT NULL,
                 json             TEXT NOT NULL,
                 analyzer_version INTEGER NOT NULL,
                 created_at       INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE INDEX IF NOT EXISTS idx_features_version
                 ON features (analyzer_version);",
        )
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

    pub fn hashes_needing_analysis(&self, hashes: &[String]) -> rusqlite::Result<Vec<String>> {
        let mut needed = Vec::new();
        for hash in hashes {
            if self.get_features(hash)?.is_none() {
                needed.push(hash.clone());
            }
        }
        Ok(needed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn reports_only_uncached_hashes_as_needing_analysis() {
        let db = Db::open_in_memory().unwrap();
        db.put_features("cached", "/tmp/c.jpg", "{}").unwrap();
        let need = db
            .hashes_needing_analysis(&["cached".to_string(), "fresh".to_string()])
            .unwrap();
        assert_eq!(need, vec!["fresh".to_string()]);
    }
}
