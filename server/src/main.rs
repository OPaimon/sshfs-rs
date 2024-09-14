mod database;
mod auth;
mod audit;
mod fs;
mod sftp_server;

use std::{env, error::Error};
use rusqlite::Result;
use russh::server::Server;
use russh_keys::key::KeyPair;
use std::sync::Arc;
use log::LevelFilter;
use std::time::Duration;

use crate::database::{DatabasePool, GlobalDatabasePool};


#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Debug)
        .init();

    let config = russh::server::Config {
        auth_rejection_time: Duration::from_secs(3),
        auth_rejection_time_initial: Some(Duration::from_secs(0)),
        keys: vec![KeyPair::generate_ed25519().unwrap()],
        ..Default::default()
    };

    let mut server = crate::sftp_server::Server::<GlobalDatabasePool> {
        pool: GlobalDatabasePool::get_pool(),
        _marker: std::marker::PhantomData,
    };

    server
        .run_on_address(
            Arc::new(config),
            (
                "0.0.0.0",
                std::env::var("PORT")
                    .unwrap_or("2002".to_string())
                    .parse()
                    .unwrap(),
            )
        )
        .await
        .unwrap();
}