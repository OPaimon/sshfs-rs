use fuser::{FileAttr, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use itertools::Itertools;
use libc::ENOENT;
use log::warn;
use ssh2::{ErrorCode, File, OpenFlags, OpenType, Session, Sftp};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

fn convert_file_type(file_type: ssh2::FileType) -> fuser::FileType {
    match file_type {
        ssh2::FileType::NamedPipe => fuser::FileType::NamedPipe,
        ssh2::FileType::CharDevice => fuser::FileType::CharDevice,
        ssh2::FileType::BlockDevice => fuser::FileType::BlockDevice,
        ssh2::FileType::Directory => fuser::FileType::Directory,
        ssh2::FileType::RegularFile => fuser::FileType::RegularFile,
        ssh2::FileType::Symlink => fuser::FileType::Symlink,
        ssh2::FileType::Socket => fuser::FileType::Socket,
        ssh2::FileType::Other(_) => fuser::FileType::RegularFile,
    }
}

#[derive(Default)]
struct Inodes {
    list: HashMap<u64, PathBuf>,
    // 远程路径
    max_inode: u64,
}

impl Inodes {
    fn add(&mut self, path: &Path) -> u64 {
        match self.get_inode(path) {
            Some(inode) => return inode,
            None => {
                self.max_inode += 1;
                self.list.insert(self.max_inode, path.to_path_buf());
                self.max_inode
            }
        }
    }
    fn get_inode(&self, path: &Path) -> Option<u64> {
        self.list.iter().find(|(_, p)| path == *p).map(|(i, _)| *i)
    }
    fn get_path(&self, inode: u64) -> Option<PathBuf> {
        self.list.get(&inode).map(|p| p.clone())
    }
    fn del_inode(&mut self, inode: u64) -> Option<u64> {
        self.list.remove(&inode).map(|_| inode)
    }
    fn rename(&mut self, old_path: &Path, new_path: &Path) -> bool {
        let inode = self.get_inode(old_path);
        if inode.is_none() {
            return false;
        }
        let inode = inode.unwrap();
        self.list.insert(inode, new_path.to_path_buf());
        true
    }
}

pub struct sshfs {
    _session: Session,
    _root_path: PathBuf,
    sftp: Sftp,
    inodes: Inodes,
    file_handles: HashMap<u64, File>,
}

impl sshfs {
    pub fn new(session: Session, root_path: PathBuf) -> Self {
        let mut inodes = Inodes::default();
        let sftp = session.sftp().unwrap();
        inodes.add(&root_path);
        Self {
            _session: session,
            _root_path: root_path,
            sftp,
            inodes,
            file_handles: HashMap::new(),
        }
    }
    fn get_attr(&mut self, path: &Path) -> Result<FileAttr, ErrorCode> {
        let stat = self.sftp.stat(path).unwrap();
        let ino = self.inodes.add(path);
        let kind = stat.file_type();
        Ok(FileAttr {
            ino,
            size: stat.size.unwrap_or(0),
            blksize: 1024,
            blocks: stat.size.unwrap_or(0) / 1024 + 1,
            atime: UNIX_EPOCH + Duration::from_secs(stat.atime.unwrap_or(0)),
            mtime: UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0)),
            ctime: UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0)),
            crtime: UNIX_EPOCH + Duration::from_secs(stat.mtime.unwrap_or(0)),
            // 就当是这样吧，反正我确实找不到创建时间
            kind: convert_file_type(kind),
            perm: stat.perm.unwrap_or(0o666) as u16,
            nlink: 1,
            uid: stat.uid.unwrap_or(0),
            gid: stat.gid.unwrap_or(0),
            rdev: 1,
            flags: 0,
        })
    }
}

impl Filesystem for sshfs {
    fn lookup(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: ReplyEntry,
    ) {
        let Some(mut path) = self.inodes.get_path(parent) else {
            reply.error(ENOENT);
            return;
        };
        path.push(Path::new(name));
        match self.get_attr(path.as_path()) {
            Ok(attr) => {
                reply.entry(&Duration::new(1, 0), &attr, 0);
            }
            Err(_) => {
                reply.error(ENOENT);
            }
        }
    }
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        match self.inodes.get_path(ino) {
            Some(path) => match self.get_attr(path.as_path()) {
                Ok(attr) => {
                    reply.attr(&Duration::new(1, 0), &attr);
                }
                Err(_) => {
                    reply.error(ENOENT);
                }
            },
            None => {
                reply.error(ENOENT);
            }
        }
    }
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        match self.inodes.get_path(ino) {
            Some(path) => match self.sftp.readdir(&path) {
                Ok(mut dir) => {
                    let cur_file_attr = ssh2::FileStat {
                        size: None,
                        uid: None,
                        gid: None,
                        perm: Some(libc::S_IFDIR),
                        atime: None,
                        mtime: None,
                    };
                    dir.insert(0, (Path::new(".").into(), cur_file_attr.clone()));
                    dir.insert(0, (Path::new("..").into(), cur_file_attr));
                    let _ = dir
                        .iter()
                        .skip(offset as usize)
                        .enumerate()
                        .map(|(i, f)| {
                            let ino = if f.0 == Path::new(".") || f.0 == Path::new("..") {
                                1
                            } else {
                                self.inodes.add(&f.0)
                            };
                            let name = match f.0.file_name() {
                                Some(name) => name,
                                None => f.0.as_os_str(),
                            };
                            let kind = convert_file_type(f.1.file_type());
                            (ino, i + offset as usize + 1, kind, name)
                        })
                        .take_while(|(ino, i, filetype, name)| {
                            !reply.add(*ino, *i as i64, *filetype, name)
                        })
                        .collect::<Vec<_>>();
                    reply.ok()
                }
                Err(e) => {
                    warn!("Failed to read directory: {}", e);
                    reply.error(ENOENT);
                }
            },
            None => {
                reply.error(ENOENT)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inodes() {
        let mut inodes = Inodes::default();
        let path = Path::new("/test");
        let inode = inodes.add(path);
        assert_eq!(inodes.get_inode(path), Some(inode));
        assert_eq!(inodes.get_path(inode), Some(path.to_path_buf()));
        assert_eq!(inodes.del_inode(inode), Some(inode));
        assert_eq!(inodes.get_inode(path), None);
        assert_eq!(inodes.get_path(inode), None);
        assert_eq!(inodes.del_inode(inode), None);
        let path = Path::new("/test");
        let inode = inodes.add(path);
        let new_path = Path::new("/new_test");
        assert_eq!(inodes.rename(path, new_path), true);
        assert_eq!(inodes.get_inode(path), None);
        assert_eq!(inodes.get_inode(new_path), Some(inode));
        assert_eq!(inodes.get_path(inode), Some(new_path.to_path_buf()));
    }
}
