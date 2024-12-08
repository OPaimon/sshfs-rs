use anyhow::Result;
use fuser::MountOption;
pub mod sftp_client;
pub mod filesystem;


/// Configuration for mounting a remote filesystem over SSH.
///
/// # Fields
///
/// - `addr`: The address of the SSH server (e.g., `"example.com:22"`).
/// - `username`: The username for SSH authentication.
/// - `password`: The password for the SSH user.
/// - `path`: The remote directory to mount (e.g., `"/var/www"`).
/// - `mountpoint`: The local directory where the remote filesystem will be mounted.
/// - `auto_unmount`: If `true`, automatically unmount the filesystem when the program exits.
/// - `allow_root`: If `true`, allow root user access to the mounted filesystem.
pub struct SshFsConfig {
    pub addr: String,
    pub username: String,
    pub password: String,
    pub path: String,
    pub mountpoint: String,
    pub auto_unmount: bool,
    pub allow_root: bool,
}

pub fn mount_sshfs(config: SshFsConfig) -> Result<()> {
    let session = sftp_client::make_ssh_session_by_password(
        &config.username,
        &config.password,
        &config.addr,
    )?;
    let fs = filesystem::Sshfs::new(session, config.path.into());
    let mut options = vec![
        MountOption::RW,
        MountOption::FSName("sshfs-rs".to_string()),
    ];
    if config.auto_unmount {
        options.push(MountOption::AutoUnmount);
    }
    if config.allow_root {
        options.push(MountOption::AllowRoot);
    }
    fuser::mount2(fs, &config.mountpoint, &options)?;
    Ok(())
}