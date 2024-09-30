mod sftp_client;
mod filesystem;

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgAction, Command};
use fuser::MountOption;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("sshfs")
        .author("OPaimoe")
        .about("Mount an SFTP filesystem using FUSE")
        .arg(
            Arg::new("MOUNT_POINT")
                .required(true)
                .index(1)
                .help("Path where the filesystem will be mounted"),
        )
        .arg(
            Arg::new("addr")
                .long("addr")
                .short('a')
                .required(true)
                .value_name("ADDR")
                .help("SFTP server address (e.g., localhost:22)"),
        )
        .arg(
            Arg::new("username")
                .long("username")
                .short('u')
                .required(true)
                .value_name("USERNAME")
                .help("Username for the SFTP server"),
        )
        .arg(
            Arg::new("password")
                .long("password")
                .short('p')
                .required(true)
                .value_name("PASSWORD")
                .help("Password for the SFTP server"),
        )
        .arg(
            Arg::new("path")
                .long("path")
                .short('P')
                .default_value("/")
                .value_name("PATH")
                .help("Path on the SFTP server to mount"),
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
    let addr = matches.get_one::<String>("addr").unwrap();
    let username = matches.get_one::<String>("username").unwrap();
    let password = matches.get_one::<String>("password").unwrap();
    let path = "/";
    let session = sftp_client::make_ssh_session_by_password(username, password, addr)?;
    let fs = filesystem::sshfs::new(session, path.into());
    env_logger::init();
    let mountpoint = matches.get_one::<String>("MOUNT_POINT").unwrap();
    let mut options = vec![MountOption::RW, MountOption::FSName("sshfs-rs".to_string())];
    if matches.get_flag("auto_unmount") {
        options.push(MountOption::AutoUnmount);
    }
    if matches.get_flag("allow-root") {
        options.push(MountOption::AllowRoot);
    }
    fuser::mount2(fs, mountpoint, &options).unwrap();

    Ok(())
}