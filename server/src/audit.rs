use std::sync::Arc;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use tracing::{Event, Subscriber};
use tracing_subscriber::{layer::Context, Layer};


// 自定义 Layer，将日志插入到 AuditLogs 表中
pub struct DatabaseLogger {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl DatabaseLogger {
    pub fn new(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        DatabaseLogger { pool }
    }
}

// 实现 tracing::Layer trait
impl<S> Layer<S> for DatabaseLogger
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        // 获取数据库连接
        let conn = self.pool.get().expect("Failed to get connection from pool");

        // 提取事件的元数据和字段（如 username、action、target 等）
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        match visitor.get_val() {
            Some((username, action, target)) => {
                // 插入日志数据到 AuditLogs 表
                if let Err(e) = log_action_to_audit_logs(&conn, &username, &action, &target) {
                    eprintln!("Failed to log action to database: {:?}", e);
                }
            }
            None => {
                eprintln!("Event is missing required fields: username, action, or target");
            }
        }
    }
}

// 定义一个 Visitor 来提取事件中的字段
#[derive(Default)]
struct LogVisitor {
    username: Option<String>,
    action: Option<String>,
    target: Option<String>,
}

impl LogVisitor {
    /// 检查所有字段是否都存在
    fn is_valid(&self) -> bool {
        self.username.is_some() && self.action.is_some() && self.target.is_some()
    }

    fn get_val(&self) -> Option<(String, String, String)> {
        if self.is_valid() {
            Some((
                self.username.as_ref().unwrap().to_string(),
                self.action.as_ref().unwrap().to_string(),
                self.target.as_ref().unwrap().to_string(),
            ))
        } else {
            None
        }
    }
}

impl tracing::field::Visit for LogVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "username" => self.username = Some(value.to_string()),
            "action" => self.action = Some(value.to_string()),
            "target" => self.target = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "target" {
            self.target = Some(format!("{:?}", value));
        }
    }
}

/// 将日志写入到 AuditLogs 表的函数
fn log_action_to_audit_logs(conn: &Connection, username: &str, action: &str, target: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO AuditLogs (username, action, target) VALUES (?1, ?2, ?3)",
        params![username, action, target],
    )?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::database::{MockDatabasePool, DatabasePool};

    #[test]
    fn test_log_action_to_audit_logs() {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::new(manager).expect("Failed to create database connection pool");
        let conn = pool.get().expect("Failed to get connection from pool");

        conn.execute(
            "CREATE TABLE AuditLogs (
                log_id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT NOT NULL,
                action TEXT NOT NULL,
                target TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            params![],
        )
        .expect("Failed to create AuditLogs table");

        log_action_to_audit_logs(&conn, "test", "read", "file.txt").unwrap();

        let mut stmt = conn
            .prepare("SELECT * FROM AuditLogs WHERE username = ? AND action = ? AND target = ?")
            .unwrap();
        let log = stmt
            .query_row(params!["test", "read", "file.txt"], |row| {
                let username: String = row.get(1)?;
                let action: String = row.get(2)?;
                let target: String = row.get(3)?;
                Ok((username, action, target))
            })
            .unwrap();
        assert_eq!(log.0, "test");
        assert_eq!(log.1, "read");
        assert_eq!(log.2, "file.txt");
    }

    #[test]
    fn test_database_logger() {
        let _manager = SqliteConnectionManager::memory();
        let pool = MockDatabasePool::get_pool();

        let logger = DatabaseLogger::new(pool.clone());
        let subscriber = tracing_subscriber::Registry::default().with(logger);

        tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");
        tracing::info!(username = "admin", action = "read", target = "file.txt", "test log");

        let conn = pool.get().expect("Failed to get connection from pool");
        let mut stmt = conn
            .prepare("SELECT * FROM AuditLogs WHERE username = ? AND action = ? AND target = ?")
            .unwrap();
        let log = stmt
            .query_row(params!["admin", "read", "file.txt"], |row| {
                let username: String = row.get(1)?;
                let action: String = row.get(2)?;
                let target: String = row.get(3)?;
                Ok((username, action, target))
            })
            .unwrap();
        assert_eq!(log.0, "admin");
        assert_eq!(log.1, "read");
        assert_eq!(log.2, "file.txt");
    }
}