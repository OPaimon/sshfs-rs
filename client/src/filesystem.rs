use fuser::{FileAttr, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, Request};
use libc::ENOENT;
use log::warn;
use ssh2::{ErrorCode, File, OpenFlags, OpenType, Session, Sftp};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
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

fn conver_open_flag(flags: i32) -> OpenFlags {
    let mut flags_ssh2 = OpenFlags::empty();
    if flags & libc::O_WRONLY != 0 {
        flags_ssh2.insert(OpenFlags::WRITE);
    } else if flags & libc::O_RDWR != 0 {
        flags_ssh2.insert(OpenFlags::READ);
        flags_ssh2.insert(OpenFlags::WRITE);
    } else {
        flags_ssh2.insert(OpenFlags::READ);
    }
    if flags & libc::O_APPEND != 0 {
        flags_ssh2.insert(OpenFlags::APPEND);
    }
    if flags & libc::O_CREAT != 0 {
        flags_ssh2.insert(OpenFlags::CREATE);
    }
    if flags & libc::O_TRUNC != 0 {
        flags_ssh2.insert(OpenFlags::TRUNCATE);
    }
    if flags & libc::O_EXCL != 0 {
        flags_ssh2.insert(OpenFlags::EXCLUSIVE);
    }
    flags_ssh2
}

struct Error(i32);

impl From<ErrorCode> for Error {
    fn from(e: ErrorCode) -> Self {
        let result = match e {
            ssh2::ErrorCode::Session(_) => libc::ENXIO,
            ssh2::ErrorCode::SFTP(i) => match i {
                // libssh2のlibssh2_sftp.hにて定義されている。
                2 => libc::ENOENT,        // NO_SUCH_FILE
                3 => libc::EACCES,        // permission_denied
                4 => libc::EIO,           // failure
                5 => libc::ENODEV,        // bad message
                6 => libc::ENXIO,         // no connection
                7 => libc::ENETDOWN,      // connection lost
                8 => libc::ENODEV,        // unsported
                9 => libc::EBADF,         // invalid handle
                10 => libc::ENOENT,       //no such path
                11 => libc::EEXIST,       // file already exists
                12 => libc::EACCES,       // write protected
                13 => libc::ENXIO,        // no media
                14 => libc::ENOSPC,       // no space on filesystem
                15 => libc::EDQUOT,       // quota exceeded
                16 => libc::ENODEV,       // unknown principal
                17 => libc::ENOLCK,       // lock conflict
                18 => libc::ENOTEMPTY,    // dir not empty
                19 => libc::ENOTDIR,      // not a directory
                20 => libc::ENAMETOOLONG, // invalid file name
                21 => libc::ELOOP,        // link loop
                _ => 0,
            },

        };
        Error(result)
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
            Some(inode) => inode,
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
        self.list.get(&inode).cloned()
    }
    fn del_inode(&mut self, inode: u64) -> Option<u64> {
        self.list.remove(&inode).map(|_| inode)
    }
    fn _rename(&mut self, old_path: &Path, new_path: &Path) -> bool {
        let inode = self.get_inode(old_path);
        if inode.is_none() {
            return false;
        }
        let inode = inode.unwrap();
        self.list.insert(inode, new_path.to_path_buf());
        true
    }
}

#[derive(Default)]
struct FHandlers {
    list: HashMap<u64, File>,
    next_fh: u64,
}

impl FHandlers {
    fn add(&mut self, file: File) -> u64 {
        self.list.insert(self.next_fh, file);
        self.next_fh += 1;
        self.next_fh - 1
    }
    fn get(&mut self, fh: u64) -> Option<&mut File> {
        self.list.get_mut(&fh)
    }
    fn del(&mut self, fh: u64) -> Option<File> {
        self.list.remove(&fh)
    }
}

pub struct Sshfs {
    _session: Session,
    _root_path: PathBuf,
    sftp: Sftp,
    inodes: Inodes,
    file_handles: FHandlers,
}

impl Sshfs {
    pub fn new(session: Session, root_path: PathBuf) -> Self {
        let mut inodes = Inodes::default();
        let sftp = session.sftp().unwrap();
        inodes.add(&root_path);
        Self {
            _session: session,
            _root_path: root_path,
            sftp,
            inodes,
            file_handles: FHandlers::default(),
        }
    }
    fn get_attr(&mut self, path: &Path) -> Result<FileAttr, ErrorCode> {
        match self.sftp.stat(path) {
            Ok(stat) => {
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
            Err(e) => Err(e.code()),
        }
    }
}

impl Filesystem for Sshfs {
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
                Err(e) => {
                    reply.error(Error::from(e).0);
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
                    reply.error(Error::from(e.code()).0);
                }
            },
            None => reply.error(ENOENT),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, flags: i32, reply: fuser::ReplyOpen) {
        match self.inodes.get_path(ino) {
            None => {
                reply.error(ENOENT);
            }
            Some(path) => {
                let flags_ssh2 = conver_open_flag(flags);
                match self.sftp.open_mode(&path, flags_ssh2,0o777, OpenType::File) {
                    Ok(file) => {
                        let fh = self.file_handles.add(file);
                        reply.opened(fh, flags as u32);
                    }
                    Err(e) => {
                        warn!("Failed to open file: {}", e);
                        reply.error(Error::from(e.code()).0);
                    }
                }
            }
        }
    }

    fn release(
            &mut self,
            _req: &Request<'_>,
            _ino: u64,
            fh: u64,
            _flags: i32,
            _lock_owner: Option<u64>,
            _flush: bool,
            reply: fuser::ReplyEmpty,
        ) {
        self.file_handles.del(fh);
        reply.ok();
    }

    fn read(
            &mut self,
            _req: &Request<'_>,
            _ino: u64,
            fh: u64,
            offset: i64,
            size: u32,
            _flags: i32,
            _lock_owner: Option<u64>,
            reply: ReplyData,
        ) {
        let file = match self.file_handles.get(fh) {
            Some(file) => file,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };
        match file.seek(std::io::SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                let mut buf = vec![0; size as usize];
                let mut read_size: usize = 0;
                while read_size < size as usize {
                    match file.read(&mut buf[read_size..]) {
                        Ok(n) => {
                            if n == 0 {
                                break;
                            }
                            read_size += n;
                        }
                        Err(e) => {
                            warn!("Failed to read file: {}", e);
                            reply.error(ENOENT);
                            return;
                        }
                    }
                }
                buf.truncate(read_size);
                buf.resize(read_size, 0u8);
                reply.data(&buf);
            }
            Err(e) => {
                warn!("Failed to seek file: {}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn write(
            &mut self,
            _req: &Request<'_>,
            _ino: u64,
            fh: u64,
            offset: i64,
            data: &[u8],
            _write_flags: u32,
            _flags: i32,
            _lock_owner: Option<u64>,
            reply: fuser::ReplyWrite,
        ) {
        let file = match self.file_handles.get(fh) {
            Some(file) => file,
            None => {
                reply.error(libc::EINVAL);
                return;
            }
        };
        match file.seek(std::io::SeekFrom::Start(offset as u64)) {
            Ok(_) => {
                let mut buf = data;
                while !buf.is_empty() {
                    match file.write(buf) {
                        Ok(n) => {
                            buf = &buf[n..];
                        }
                        Err(e) => {
                            warn!("Failed to write file: {}", e);
                            reply.error(ENOENT);
                            return;
                        }
                    }
                }
                reply.written(data.len() as u32);
            }
            Err(e) => {
                warn!("Failed to seek file: {}", e);
                reply.error(ENOENT);
            }
        }
    }

    fn mknod(
            &mut self,
            _req: &Request<'_>,
            parent: u64,
            name: &std::ffi::OsStr,
            mode: u32,
            umask: u32,
            _rdev: u32,
            reply: ReplyEntry,
        ) {
        if mode & libc::S_IFMT != libc::S_IFREG {
            reply.error(libc::EPERM);
            return;
        }
        match self.inodes.get_path(parent) {
            None => {
                reply.error(ENOENT);
            }
            Some(parent_path) => {
                let mut path = parent_path.clone();
                path.push(Path::new(name));
                let mode = mode & (!umask | libc::S_IFMT); //只保留文件类型
                match self.sftp.open_mode(&path, OpenFlags::CREATE, mode as i32, OpenType::File) {
                    Ok(_) => {
                        match self.get_attr(path.as_path()) {
                            Ok(attr) => {
                                reply.entry(&Duration::new(1, 0), &attr, 0);
                            }
                            Err(e) => {
                                reply.error(Error::from(e).0);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create file: {}", e);
                        reply.error(Error::from(e.code()).0);
                    }
                }
            }
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &std::ffi::OsStr, reply: fuser::ReplyEmpty) {
        match self.inodes.get_path(parent) {
            None => {
                reply.error(ENOENT);
            }
            Some(parent_path) => {
                let mut path = parent_path.clone();
                path.push(Path::new(name));
                match self.sftp.unlink(&path) {
                    Ok(_) => {
                        let ino = self.inodes.get_inode(&path).unwrap();
                        self.inodes.del_inode(ino);
                        reply.ok();
                    }
                    Err(e) => {
                        warn!("Failed to remove file: {}", e);
                        reply.error(Error::from(e.code()).0);
                    }
                }
            }
        }
    }

    fn mkdir(
            &mut self,
            _req: &Request<'_>,
            parent: u64,
            name: &std::ffi::OsStr,
            mode: u32,
            umask: u32,
            reply: ReplyEntry,
        ) {
        match self.inodes.get_path(parent) {
            None => {
                reply.error(ENOENT);
            }
            Some(parent_path) => {
                let mut path = parent_path.clone();
                path.push(Path::new(name));
                let mode = mode & (!umask | libc::S_IFMT); //只保留文件类型
                match self.sftp.mkdir(&path, mode as i32) {
                    Ok(_) => {
                        match self.get_attr(path.as_path()) {
                            Ok(attr) => {
                                reply.entry(&Duration::new(1, 0), &attr, 0);
                            }
                            Err(e) => {
                                reply.error(Error::from(e).0);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create directory: {}", e);
                        reply.error(Error::from(e.code()).0);
                    }
                }
            }
        }
    }

    fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &std::ffi::OsStr, reply: fuser::ReplyEmpty) {
        match self.inodes.get_path(parent) {
            None => {
                reply.error(ENOENT);
            }
            Some(parent_path) => {
                let mut path = parent_path.clone();
                path.push(Path::new(name));
                match self.sftp.rmdir(&path) {
                    Ok(_) => {
                        let ino = self.inodes.get_inode(&path).unwrap();
                        self.inodes.del_inode(ino);
                        reply.ok();
                    }
                    Err(e) => {
                        warn!("Failed to remove directory: {}", e);
                        reply.error(Error::from(e.code()).0);
                    }
                }
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
        assert!(inodes._rename(path, new_path));
        assert_eq!(inodes.get_inode(path), None);
        assert_eq!(inodes.get_inode(new_path), Some(inode));
        assert_eq!(inodes.get_path(inode), Some(new_path.to_path_buf()));
    }
}
