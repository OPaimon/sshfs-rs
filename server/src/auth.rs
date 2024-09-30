use crate::database::{DatabasePool, GlobalDatabasePool};
use anyhow::Result;
use bcrypt::{hash, verify};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::sync::Arc;

pub trait Auther {
    fn register(&self, username: &str, password: &str) -> Result<()>;
    fn authenticate(&mut self, username: &str, password: &str) -> Result<()>;
    fn check_permission(&self, username: &str, permission: &str) -> Result<bool>;
    fn update_user_password(
        &self,
        username: &str,
        password: &str,
        old_password: &str,
    ) -> Result<()>;
}

pub struct User<P: DatabasePool> {
    pool: Arc<Pool<SqliteConnectionManager>>,
    _marker: std::marker::PhantomData<P>,
    pub username: String,
    authed: bool,
}

unsafe impl<P: DatabasePool> Send for User<P> {}

impl<P: DatabasePool> User<P> {
    pub fn new() -> Result<Self> {
        let pool = P::get_pool();
        Ok(Self {
            pool,
            _marker: std::marker::PhantomData,
            username: String::new(),
            authed: false,
        })
    }
    pub fn new_with_pool(pool: Arc<Pool<SqliteConnectionManager>>) -> Self {
        Self {
            pool,
            _marker: std::marker::PhantomData,
            username: String::new(),
            authed: false,
        }
    }
}

impl<P: DatabasePool> Auther for User<P> {
    fn register(&self, username: &str, password: &str) -> Result<()> {
        let conn = self.pool.get()?;
        let hashed_password = hash(password, bcrypt::DEFAULT_COST)?;
        conn.execute(
            "INSERT INTO Users (username, password, role) VALUES (?, ?, ?)",
            params![username, hashed_password, "user"],
        )?;
        Ok(())
    }
    fn authenticate(&mut self, username: &str, password: &str) -> Result<()> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT password FROM Users WHERE username = ?")?;
        let hash: String = stmt.query_row(params![username], |row| row.get(0))?;
        if verify(password, &hash)? {
            self.username = username.to_string();
            self.authed = true;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Invalid password"))
        }
    }
    fn check_permission(&self, username: &str, permission: &str) -> Result<bool> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT role FROM Users WHERE username = ?")?;
        let role: String = stmt.query_row(params![username], |row| row.get(0))?;
        Ok(role == permission)
    }
    fn update_user_password(
        &self,
        username: &str,
        password: &str,
        old_password: &str,
    ) -> Result<()> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT password FROM Users WHERE username = ?")?;
        let hash_old: String = stmt.query_row(params![username], |row| row.get(0))?;
        if verify(old_password, &hash_old)? {
            let hashed_password = hash(password, bcrypt::DEFAULT_COST)?;
            conn.execute(
                "UPDATE Users SET password = ? WHERE username = ?",
                params![hashed_password, username],
            )?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Invalid password"))
        }
    }
}

// Test
#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::MockDatabasePool;
    use rusqlite::params;

    #[test]
    fn test_register() {
        let pool = MockDatabasePool::get_pool();
        let conn = pool.get().unwrap();
        let auth = User::<MockDatabasePool>::new_with_pool(pool);
        auth.register("test", "password").unwrap();
        let mut stmt = conn
            .prepare("SELECT * FROM Users WHERE username = ?")
            .unwrap();
        let user = stmt
            .query_row(params!["test"], |row| {
                let username: String = row.get(1)?;
                let password: String = row.get(2)?;
                Ok((username, password))
            })
            .unwrap();
        assert_eq!(user.0, "test");
        assert!(bcrypt::verify("password", &user.1).unwrap());
    }

    #[test]
    fn test_authenticate() {
        let pool = MockDatabasePool::get_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO Users (username, password, role) VALUES (?, ?, ?)",
            params![
                "test",
                bcrypt::hash("password", bcrypt::DEFAULT_COST).unwrap(),
                "user"
            ],
        )
        .unwrap();
        let mut auth = User::<MockDatabasePool>::new_with_pool(pool);
        auth.authenticate("test", "password").unwrap();
    }

    #[test]
    fn test_check_permission() {
        let pool = MockDatabasePool::get_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO Users (username, password, role) VALUES (?, ?, ?)",
            params![
                "test",
                bcrypt::hash("password", bcrypt::DEFAULT_COST).unwrap(),
                "user"
            ],
        )
        .unwrap();
        let auth = User::<MockDatabasePool>::new_with_pool(pool);
        assert!(auth.check_permission("test", "user").unwrap());
        assert!(!auth.check_permission("test", "admin").unwrap());
    }

    #[test]
    fn test_update_user_password() {
        let pool = MockDatabasePool::get_pool();
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO Users (username, password, role) VALUES (?, ?, ?)",
            params![
                "test",
                bcrypt::hash("password", bcrypt::DEFAULT_COST).unwrap(),
                "user"
            ],
        )
        .unwrap();
        let auth = User::<MockDatabasePool>::new_with_pool(pool);
        auth.update_user_password("test", "new_password", "password")
            .unwrap();
        let mut stmt = conn
            .prepare("SELECT password FROM Users WHERE username = ?")
            .unwrap();
        let hash: String = stmt.query_row(params!["test"], |row| row.get(0)).unwrap();
        assert!(bcrypt::verify("new_password", &hash).unwrap());
    }
}
