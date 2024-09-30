mod audit;
mod auth;
mod database;
mod fs;
mod sftp_server;

use auth::Auther;
use clap::{Arg, ArgAction, Command, Subcommand};
use log::LevelFilter;
use rusqlite::Result;
use russh::server::Server;
use russh_keys::key::KeyPair;
use std::default;
use std::sync::Arc;
use std::time::Duration;
use std::{env, error::Error};
use tracing_subscriber::layer::SubscriberExt;

use crate::audit::DatabaseLogger;
use crate::auth::User;
use crate::database::{DatabasePool, GlobalDatabasePool};

#[tokio::main]
async fn main() {
    // pub trait Auther {
    //     fn register(&self, username: &str, password: &str) -> Result<()>;
    //     fn authenticate(&mut self, username: &str, password: &str) -> Result<()>;
    //     fn check_permission(&self, username: &str, permission: &str) -> Result<bool>;
    //     fn update_user_password(
    //         &self,
    //         username: &str,
    //         password: &str,
    //         old_password: &str,
    //     ) -> Result<()>;
    // }
    let matches = Command::new("sftp-server")
        .author("OPaimoe")
        .about("An SFTP server")
        .subcommand(
            Command::new("run")
                .about("Run the SFTP server")
                .arg(
                    Arg::new("port")
                        .long("port")
                        .short('p')
                        .value_name("PORT")
                        .help("Port to listen on"),
                )
        )
        .subcommand(
            Command::new("auth")
                .about("Manage users")
                .subcommand(
                    Command::new("register")
                        .about("Register a new user")
                        .arg(
                            Arg::new("username")
                                .required(true)
                                .index(1)
                                .help("Username for the new user"),
                        )
                        .arg(
                            Arg::new("password")
                                .required(true)
                                .index(2)
                                .help("Password for the new user"),
                        ),
                )
                .subcommand(
                    Command::new("update-password")
                        .about("Update a user's password")
                        .arg(
                            Arg::new("username")
                                .required(true)
                                .index(1)
                                .help("Username of the user to update"),
                        )
                        .arg(
                            Arg::new("password")
                                .required(true)
                                .index(2)
                                .help("New password for the user"),
                        )
                        .arg(
                            Arg::new("old-password")
                                .required(true)
                                .index(3)
                                .help("Old password for the user"),
                        ),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("run", run_matches)) => {
            let default_port = env::var("PORT").unwrap_or("22".to_string());
            let port = run_matches.get_one::<String>("port").unwrap_or(&default_port);
            let port = port.parse::<u16>().unwrap();
            env_logger::builder()
                .filter_level(LevelFilter::Debug)
                .init();

            let config = russh::server::Config {
                auth_rejection_time: Duration::from_secs(3),
                auth_rejection_time_initial: Some(Duration::from_secs(0)),
                keys: vec![KeyPair::generate_ed25519().unwrap()],
                ..Default::default()
            };

            let logger = DatabaseLogger::new(GlobalDatabasePool::get_pool());
            let subscriber = tracing_subscriber::Registry::default().with(logger);
            tracing::subscriber::set_global_default(subscriber).expect("Failed to set subscriber");

            let mut server = crate::sftp_server::Server::<GlobalDatabasePool> {
                pool: GlobalDatabasePool::get_pool(),
                _marker: std::marker::PhantomData,
            };

            server
                .run_on_address(Arc::new(config), ("0.0.0.0", port))
                .await
                .unwrap();
        }
        Some(("auth", auth_matches)) => {
            let pool = GlobalDatabasePool::get_pool();
            let auth = User::<GlobalDatabasePool>::new_with_pool(pool);
            match auth_matches.subcommand() {
                Some(("register", register_matches)) => {
                    let username = register_matches.get_one::<String>("username").unwrap();
                    let password = register_matches.get_one::<String>("password").unwrap();
                    auth.register(username, password).unwrap();
                    ()
                }
                Some(("update-password", update_password_matches)) => {
                    let username = update_password_matches
                        .get_one::<String>("username")
                        .unwrap();
                    let password = update_password_matches
                        .get_one::<String>("password")
                        .unwrap();
                    let old_password = update_password_matches
                        .get_one::<String>("old-password")
                        .unwrap();
                    auth.update_user_password(username, password, old_password)
                        .unwrap();
                    ()
                }
                _ => {}
            }
        }
        _ => {}
    }
}
