use anyhow::{Context, Result};
use lazy_static::lazy_static;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use std::{env, sync::Arc};

// Define a trait for the database connection pool
pub trait DatabasePool {
    fn get_pool() -> Arc<Pool<SqliteConnectionManager>>;
}

pub struct GlobalDatabasePool;

// Implement the trait for the specific type
impl DatabasePool for GlobalDatabasePool {
    fn get_pool() -> Arc<Pool<SqliteConnectionManager>> {
        GLOBAL_DB_POOL.clone()
    }
}

pub struct MockDatabasePool;

impl DatabasePool for MockDatabasePool {
    fn get_pool() -> Arc<Pool<SqliteConnectionManager>> {
        // Create a mock pool
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::new(manager).expect("Failed to create database connection pool");

        // Initialize the mock database
        let conn = pool.get().expect("Failed to get connection from pool");
        initialize_database(&conn).expect("Failed to initialize database");

        Arc::new(pool)
    }
}

fn initialize_database(conn: &Connection) -> Result<()> {
    // 检查 Users 表是否存在
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='Users'")
        .context("Failed to prepare statement to check for Users table")?;

    let users_table_exists: bool = stmt.exists(params![])?;

    if !users_table_exists {
        // 创建 Users 表
        conn.execute(
            "CREATE TABLE Users (
                user_id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password TEXT NOT NULL,
                role TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            params![],
        )
        .context("Failed to create Users table")?;
        let hashed_admin_password = bcrypt::hash("admin_password", bcrypt::DEFAULT_COST)?;
        // 插入初始数据
        conn.execute(
            "INSERT INTO Users (username, password, role) VALUES (?1, ?2, ?3)",
            params!["admin", hashed_admin_password, "admin"],
        )
        .context("Failed to insert initial data into Users table")?;

        // 检查 AuditLogs 表是否存在
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='AuditLogs'")
            .context("Failed to prepare statement to check for AuditLogs table")?;

        let audit_logs_table_exists: bool = stmt.exists(params![])?;

        if !audit_logs_table_exists {
            conn.execute(
            "CREATE TABLE AuditLogs (
                log_id INTEGER PRIMARY KEY AUTOINCREMENT,
                username INTEGER NOT NULL references Users(username),
                action TEXT NOT NULL CHECK(action IN ('Open', 'Close', 'Read', 'Write', 'Remove', 'OpenDir', 'ReadDir', 'MakeDir', 'RemoveDir', 'RealPath', 'Rename')),
                target TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            params![],
            )
            .context("Failed to create AuditLogs table")?;
        }
    }

    Ok(())
}

lazy_static! {
    static ref GLOBAL_DB_POOL: Arc<Pool<SqliteConnectionManager>> = {
        // 从环境变量获取数据库路径
        let db_path = env::var("DATABASE_PATH").unwrap_or("my_database.db".to_string());

        // 创建连接池管理器
        let manager = SqliteConnectionManager::file(db_path.clone());
        let pool = Pool::new(manager).expect("Failed to create database connection pool");

        let conn = Connection::open(db_path).expect("Failed to open database connection");
        initialize_database(&conn).expect("Failed to initialize database");

        Arc::new(pool)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_database() {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::new(manager).expect("Failed to create database connection pool");

        let conn = pool.get().expect("Failed to get connection from pool");
        initialize_database(&conn).expect("Failed to initialize database");
    }

    #[test]
    fn test_get_pool() {
        let pool = GlobalDatabasePool::get_pool();
        assert!(pool.get().is_ok());
    }

    #[test]
    fn test_mock_get_pool() {
        let pool = MockDatabasePool::get_pool();
        assert!(pool.get().is_ok());
    }
}
