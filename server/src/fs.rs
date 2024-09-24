use chrono::{DateTime, Local, TimeZone};
use russh_sftp::protocol::FileAttr;
use russh_sftp::protocol::FileAttributes;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, time};
use users::{get_group_by_gid, get_user_by_uid};

use russh_sftp::de;

pub struct VirtualRoot {
    virtual_root: PathBuf,
}

impl VirtualRoot {
    fn new(virtual_root: &Path) -> io::Result<Self> {
        if !virtual_root.exists() || !virtual_root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Virtual root directory does not exist or is not a directory.",
            ));
        }
        Ok(Self {
            virtual_root: virtual_root.to_path_buf(),
        })
    }

    pub fn get_root(&self) -> &Path {
        &self.virtual_root
    }

    pub fn to_virtual_path(&self, real_path: &Path) -> io::Result<PathBuf> {
        let relative_path = real_path
            .strip_prefix(&self.virtual_root)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        Ok(PathBuf::from("/").join(relative_path.to_path_buf()))
    }

    pub fn to_real_path(&self, virtual_path: &Path) -> io::Result<PathBuf> {
        let binding = PathBuf::from(virtual_path);
        let delta = binding.strip_prefix("/");
        if delta.is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path does not start with a slash.",
            ));
        }
        let delta = delta.unwrap();
        let real_path = self.virtual_root.join(delta);
        if !real_path.starts_with(&self.virtual_root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path is outside the virtual root.",
            ));
        }
        Ok(real_path)
    }

    pub fn verify_real_path(&self, real_path: &Path) -> io::Result<()> {
        if !real_path.starts_with(&self.virtual_root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path is outside the virtual root.",
            ));
        }
        Ok(())
    }
}

impl Default for VirtualRoot {
    fn default() -> Self {
        Self {
            virtual_root: std::env::current_dir().unwrap(),
        }
    }
}

fn mode_to_rwx(mode: u32) -> String {
    let mut rwx = String::new();

    // 用户权限
    rwx.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    rwx.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    rwx.push(if mode & 0o100 != 0 { 'x' } else { '-' });

    // 组权限
    rwx.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    rwx.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    rwx.push(if mode & 0o010 != 0 { 'x' } else { '-' });

    // 其他用户权限
    rwx.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    rwx.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    rwx.push(if mode & 0o001 != 0 { 'x' } else { '-' });

    rwx
}

fn format_permissions(permissions: fs::Permissions) -> String {
    let mode = permissions.mode();
    let mut rwx = mode_to_rwx(mode);
    rwx
}

fn format_time(time: SystemTime) -> String {
    // 将 SystemTime 转换为 chrono 的 DateTime<Local>
    let datetime: DateTime<Local> = time.into();

    // 格式化时间为 "Sep 13 13:12" 格式
    datetime.format("%b %d %H:%M").to_string()
}

pub fn format_file_info(path: &Path) -> std::io::Result<String> {
    let metadata = fs::metadata(path)?;
    let permissions = metadata.permissions();
    let size = metadata.len();
    let modified = metadata.modified()?;
    let file_name = path.file_name().unwrap().to_string_lossy();

    let formatted_permissions = format_permissions(permissions);
    let formatted_time = format_time(modified);

    Ok(format!(
        "{} 1 user group {} {} {}",
        formatted_permissions, size, formatted_time, file_name
    ))
}

pub fn get_file_file_attributes(
    path: &Path,
) -> Result<russh_sftp::protocol::FileAttributes, std::io::Error> {
    // 读取文件的元数据
    let metadata = fs::metadata(path)?;

    // 创建一个 FileAttributes 实例
    // 构造 FileAttributes
    let mut file_attr = russh_sftp::protocol::FileAttributes::from(&metadata);

    let user = get_user_by_uid(metadata.uid()).unwrap();
    let group = get_group_by_gid(metadata.gid()).unwrap();
    file_attr.user = Some(user.name().to_str().unwrap().to_string());
    file_attr.group = Some(group.name().to_str().unwrap().to_string());

    Ok(file_attr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_virtual_root() {
        let temp_dir = env::current_dir().unwrap();
        println!("temp_dir: {:?}", temp_dir);
        let virtual_root = VirtualRoot::new(&temp_dir).unwrap();
        println!("root: {:?}", virtual_root.get_root());
        let virtual_path = virtual_root.to_virtual_path(temp_dir.as_path()).unwrap();
        assert_eq!(virtual_path, Path::new("/"));

        let real_path = virtual_root.to_real_path(Path::new("/")).unwrap();
        println!("real_path: {:?}", real_path);
        assert_eq!(real_path, temp_dir);

        let file_path = virtual_root
            .to_virtual_path(temp_dir.join("file.txt").as_path())
            .unwrap();
        assert_eq!(file_path, Path::new("/file.txt"));

        let real_file_path = virtual_root.to_real_path(Path::new("/file.txt")).unwrap();
        assert_eq!(real_file_path, temp_dir.join("file.txt"));

        let mut file = File::create(real_file_path).unwrap();
        file.write_all(b"Hello, world!").unwrap();

        let file_path = virtual_root
            .to_virtual_path(temp_dir.join("file.txt").as_path())
            .unwrap();
        assert_eq!(file_path, Path::new("/file.txt"));

        let real_file_path = virtual_root.to_real_path(Path::new("/file.txt")).unwrap();
        assert_eq!(real_file_path, temp_dir.join("file.txt"));
    }
}
