//! Reads what the official launcher keeps, so the setup wizard can offer to take it over.
//!
//! Only plain preferences are read: favourite servers, hubs and the compatibility switch. The
//! logins are left alone on purpose: they are tokens, and moving a token from one program to
//! another is not something to do without being asked and without the player seeing it.
//! The database is opened read-only; the official launcher is never written to.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use crate::{Error, Result};

/// A favourite server of the official launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialFavorite {
    pub address: String,
    pub name: Option<String>,
}

/// The parts of the official launcher's data that this launcher can use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OfficialData {
    /// Favourite servers, the one raised last first.
    pub favorites: Vec<OfficialFavorite>,
    /// Hub addresses, the highest priority first.
    pub hubs: Vec<String>,
    /// The official launcher's "compatibility mode", when it was ever written.
    pub compat_mode: Option<bool>,
}

/// `%APPDATA%\Space Station 14\launcher\settings.db`, where the official launcher keeps its data.
pub fn default_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA").filter(|v| !v.is_empty())?;
    Some(
        Path::new(&appdata)
            .join("Space Station 14")
            .join("launcher")
            .join("settings.db"),
    )
}

/// Reads the official launcher's database. `Ok(None)` when the file is not there.
pub fn read(path: &Path) -> Result<Option<OfficialData>> {
    if !path.is_file() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    // The official launcher may be running and holding the file for a moment.
    conn.busy_timeout(Duration::from_millis(1500))?;

    let favorites = query_favorites(&conn)?;
    let hubs = query_hubs(&conn)?;
    let compat_mode = query_config_bool(&conn, "CompatMode")?;
    Ok(Some(OfficialData {
        favorites,
        hubs,
        compat_mode,
    }))
}

/// A table that an older official launcher does not have yet is no error: it just has no rows.
fn table_exists(conn: &Connection, name: &str) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(found.is_some())
}

fn query_favorites(conn: &Connection) -> Result<Vec<OfficialFavorite>> {
    if !table_exists(conn, "FavoriteServer")? {
        return Ok(Vec::new());
    }
    // `RaiseTime` was added later than the table, so an old file may not have it.
    let has_raise = conn
        .prepare("SELECT RaiseTime FROM FavoriteServer LIMIT 0")
        .is_ok();
    let sql = if has_raise {
        "SELECT Address, Name FROM FavoriteServer ORDER BY RaiseTime DESC, rowid"
    } else {
        "SELECT Address, Name FROM FavoriteServer ORDER BY rowid"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(OfficialFavorite {
            address: r.get::<_, String>(0)?,
            name: r
                .get::<_, Option<String>>(1)?
                .filter(|n| !n.trim().is_empty()),
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if !row.address.trim().is_empty() {
            out.push(row);
        }
    }
    Ok(out)
}

fn query_hubs(conn: &Connection) -> Result<Vec<String>> {
    if !table_exists(conn, "Hub")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT Address FROM Hub ORDER BY Priority")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        if !row.trim().is_empty() {
            out.push(row);
        }
    }
    Ok(out)
}

/// A `Config` value, which SQLite may hold as a number or as text.
fn query_config_bool(conn: &Connection, key: &str) -> Result<Option<bool>> {
    if !table_exists(conn, "Config")? {
        return Ok(None);
    }
    let value: Option<Value> =
        match conn.query_row("SELECT Value FROM Config WHERE Key = ?1", [key], |r| {
            r.get::<_, Value>(0)
        }) {
            Ok(v) => Some(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(Error::from(e)),
        };
    Ok(value.and_then(|v| match v {
        Value::Integer(n) => Some(n != 0),
        Value::Text(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        _ => None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bebra-official-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.db");
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let path = std::env::temp_dir().join("bebra-official-missing/settings.db");
        assert_eq!(read(&path).unwrap(), None);
    }

    #[test]
    fn favorites_hubs_and_compat_are_read() {
        let path = temp_db("full");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE FavoriteServer (Address TEXT PRIMARY KEY NOT NULL, Name TEXT, RaiseTime DATETIME);
                 CREATE TABLE Hub (Address TEXT NOT NULL PRIMARY KEY, Priority INTEGER NOT NULL UNIQUE);
                 CREATE TABLE Config (Key TEXT PRIMARY KEY NOT NULL, Value);
                 INSERT INTO FavoriteServer VALUES ('ss14s://a.test', 'Alpha', '2026-01-01');
                 INSERT INTO FavoriteServer VALUES ('ss14s://b.test', NULL, '2026-02-01');
                 INSERT INTO FavoriteServer VALUES ('ss14s://c.test', '  ', NULL);
                 INSERT INTO Hub VALUES ('https://second.test/', 1);
                 INSERT INTO Hub VALUES ('https://first.test/', 0);
                 INSERT INTO Config VALUES ('CompatMode', 1);",
            )
            .unwrap();
        }
        let data = read(&path).unwrap().unwrap();
        // The one raised last comes first; the one never raised, last.
        let addresses: Vec<_> = data.favorites.iter().map(|f| f.address.as_str()).collect();
        assert_eq!(
            addresses,
            ["ss14s://b.test", "ss14s://a.test", "ss14s://c.test"]
        );
        assert_eq!(data.favorites[1].name.as_deref(), Some("Alpha"));
        assert_eq!(data.favorites[0].name, None);
        assert_eq!(data.favorites[2].name, None, "a blank name is no name");
        assert_eq!(data.hubs, ["https://first.test/", "https://second.test/"]);
        assert_eq!(data.compat_mode, Some(true));
    }

    #[test]
    fn a_file_without_the_tables_gives_nothing() {
        let path = temp_db("empty");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE Other (X INTEGER);")
            .unwrap();
        let data = read(&path).unwrap().unwrap();
        assert_eq!(data, OfficialData::default());
    }

    #[test]
    fn compat_mode_may_be_text() {
        let path = temp_db("text");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE Config (Key TEXT PRIMARY KEY NOT NULL, Value);
                 INSERT INTO Config VALUES ('CompatMode', '0');",
            )
            .unwrap();
        }
        assert_eq!(read(&path).unwrap().unwrap().compat_mode, Some(false));
    }
}
