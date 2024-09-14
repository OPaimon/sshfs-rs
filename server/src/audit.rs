use std::sync::Arc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use anyhow::Result;

use crate::database::{DatabasePool, MockDatabasePool};

pub enum Action {
    Open,
    Close,
    Read,
    Write,
    Remove,
    OpenDir,
    ReadDir,
    MakeDir,
    RemoveDir,
    RealPath,
    Rename,
}

impl Action {
    fn to_string(&self) -> &str {
        match self {
            Action::Open => "Open",
            Action::Close => "Close",
            Action::Read => "Read",
            Action::Write => "Write",
            Action::Remove => "Remove",
            Action::OpenDir => "OpenDir",
            Action::ReadDir => "ReadDir",
            Action::MakeDir => "MakeDir",
            Action::RemoveDir => "RemoveDir",
            Action::RealPath => "RealPath",
            Action::Rename => "Rename",
        }
    }
}


pub trait auditor :Send {
    fn audit(&self, username: &str, action: Action, path: &str) -> Result<()>;
}

pub struct Audit<P: DatabasePool> {
    pool: Arc<Pool<SqliteConnectionManager>>,
    _marker: std::marker::PhantomData<P>,
}

unsafe impl<P: DatabasePool> Send for Audit<P> {}

impl<P: DatabasePool> Audit<P> {
    pub fn new() -> Result<Self> {
        let pool = P::get_pool();
        Ok(Self {
            pool,
            _marker: std::marker::PhantomData,
        })
    }
    pub fn new_with_pool(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self {
            pool,
            _marker: std::marker::PhantomData,
        }
    }
}

    // "CREATE TABLE AuditLogs (
    //     log_id INTEGER PRIMARY KEY AUTOINCREMENT,
    //     user_id INTEGER NOT NULL references Users(user_id),
    //     action TEXT NOT NULL CHECK(action IN ('Open', 'Close', 'Read', 'Write', 'Remove', 'OpenDir', 'ReadDir', 'MakeDir', 'RemoveDir', 'RealPath', 'Rename')),
    //     target TEXT NOT NULL,
    //     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    // )",

impl<P: DatabasePool> auditor for Audit<P> {
    fn audit(&self, username: &str, action: Action, path: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO AuditLogs (username, action, target) VALUES (?, ?, ?)",
            params![username, action.to_string(), path],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_audit() {
        let pool = MockDatabasePool::get_pool();
        let conn = pool.get().unwrap();
        let auditor = Audit::<MockDatabasePool>::new_with_pool(pool);
        auditor.audit("admin", Action::Open, "/").unwrap();
        let mut stmt = conn.prepare("SELECT * FROM AuditLogs").unwrap();
        let audit = stmt.query_row([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        }).unwrap();
        assert_eq!(audit, ("admin".to_string(), "Open".to_string(), "/".to_string()));
    }
}
