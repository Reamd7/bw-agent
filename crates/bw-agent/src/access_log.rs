use rusqlite::Connection;
use std::sync::Mutex;

/// A single access log entry recording an SSH key usage event.
#[derive(Debug, serde::Serialize)]
pub struct AccessLogEntry {
    pub id: i64,
    pub timestamp: String,
    pub key_fingerprint: String,
    pub key_name: String,
    pub client_exe: String,
    pub client_pid: u32,
    pub approved: bool,
}

/// SQLite-backed access log for SSH key usage.
pub struct AccessLog {
    conn: Mutex<Connection>,
}

impl AccessLog {
    /// Open (or create) the access log database at the given path.
    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let log = Self {
            conn: Mutex::new(conn),
        };
        log.init_schema()?;
        Ok(log)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let log = Self {
            conn: Mutex::new(conn),
        };
        log.init_schema()?;
        Ok(log)
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("lock poisoned");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS access_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                key_fingerprint TEXT NOT NULL,
                key_name TEXT NOT NULL,
                client_exe TEXT NOT NULL,
                client_pid INTEGER NOT NULL,
                approved INTEGER NOT NULL
            )",
        )?;
        Ok(())
    }

    /// Record an SSH key access event.
    pub fn record(
        &self,
        fingerprint: &str,
        key_name: &str,
        exe: &str,
        pid: u32,
        approved: bool,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().expect("lock poisoned");
        conn.execute(
            "INSERT INTO access_log (key_fingerprint, key_name, client_exe, client_pid, approved) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![fingerprint, key_name, exe, pid, approved as i32],
        )?;
        Ok(())
    }

    /// Query the most recent access log entries (newest first).
    pub fn query(&self, limit: u32) -> rusqlite::Result<Vec<AccessLogEntry>> {
        let conn = self.conn.lock().expect("lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, key_fingerprint, key_name, client_exe, client_pid, approved FROM access_log ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(AccessLogEntry {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                key_fingerprint: row.get(2)?,
                key_name: row.get(3)?,
                client_exe: row.get(4)?,
                client_pid: row.get(5)?,
                approved: row.get::<_, i32>(6)? != 0,
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_and_query() {
        let log = AccessLog::open_in_memory().unwrap();
        log.record("SHA256:abc", "my-key", "ssh.exe", 1234, true)
            .unwrap();
        log.record("SHA256:def", "other-key", "git.exe", 5678, false)
            .unwrap();
        let entries = log.query(10).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key_fingerprint, "SHA256:def"); // most recent first
        assert!(entries[1].approved);
    }
}
