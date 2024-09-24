mod sftp_client;
mod filesystem;

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgAction, Command};
use fuser::MountOption;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let addr = "localhost:22";
    // let username = "test";
    // let password = "test";
    let addr = "localhost:2002";
    let username = "admin";
    let password = "admin_password";
    let path = "/";
    let session = sftp_client::make_ssh_session_by_password(username, password, addr)?;
    let fs = filesystem::sshfs::new(session, path.into());

    let matches = Command::new("hello")
        .author("Christopher Berner")
        .arg(
            Arg::new("MOUNT_POINT")
                .required(true)
                .index(1)
                .help("Act as a client, and mount FUSE at given path"),
        )
        .arg(
            Arg::new("auto_unmount")
                .long("auto_unmount")
                .action(ArgAction::SetTrue)
                .help("Automatically unmount on process exit"),
        )
        .arg(
            Arg::new("allow-root")
                .long("allow-root")
                .action(ArgAction::SetTrue)
                .help("Allow root user to access filesystem"),
        )
        .get_matches();
    env_logger::init();
    let mountpoint = matches.get_one::<String>("MOUNT_POINT").unwrap();
    let mut options = vec![MountOption::RW, MountOption::FSName("hello".to_string())];
    if matches.get_flag("auto_unmount") {
        options.push(MountOption::AutoUnmount);
    }
    if matches.get_flag("allow-root") {
        options.push(MountOption::AllowRoot);
    }
    fuser::mount2(fs, mountpoint, &options).unwrap();

    Ok(())
}