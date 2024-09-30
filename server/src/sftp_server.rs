use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs, string};

use async_trait::async_trait;
use itertools::process_results;
use log::{error, LevelFilter};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use russh::keys::key::KeyPair;
use russh::server::{Auth, Msg, Server as _, Session};
use russh::{Channel, ChannelId, MethodSet};
use russh_keys::encoding::Encoding;
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, RealPath, Status, StatusCode, Version
};
use tokio::sync::Mutex;
use tracing::{debug, trace, warn, info};

use crate::auth::{Auther, User};
use crate::database::{DatabasePool, GlobalDatabasePool};
use crate::fs::{format_file_info, get_file_file_attributes, VirtualRoot};

#[derive(Clone)]
pub struct Server<P: DatabasePool + 'static> {
    pub pool: Arc<Pool<SqliteConnectionManager>>,
    pub _marker: std::marker::PhantomData<P>,
}

impl<P: DatabasePool> russh::server::Server for Server<P> {
    type Handler = SshSession<P>;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
        SshSession::<P>::default()
    }
}

pub struct SshSession<P: DatabasePool> {
    clients: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    auther: User<P>,
}

impl<P: DatabasePool> Default for SshSession<P> {
    fn default() -> Self {
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            auther: User::new_with_pool(P::get_pool()),
        }
    }
}

impl<P: DatabasePool> SshSession<P> {
    pub async fn get_channel(&mut self, channel_id: ChannelId) -> Channel<Msg> {
        let mut clients = self.clients.lock().await;
        clients.remove(&channel_id).unwrap()
    }
}

#[async_trait]
impl<P: DatabasePool> russh::server::Handler for SshSession<P> {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        info!("credentials: {}, {}", user, password);
        match self.auther.authenticate(user, password).is_ok() {
            true => Ok(Auth::Accept),
            false => Ok(Auth::Reject {
                proceed_with_methods: Some(MethodSet::PASSWORD),
            }),
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        {
            let mut clients = self.clients.lock().await;
            clients.insert(channel.id(), channel);
        }
        Ok(true)
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        info!("subsystem: {}", name);

        if name == "sftp" {
            let channel = self.get_channel(channel_id).await;
            let username = self.auther.username.clone();
            let sftp = SftpSession::new_with_username(username);
            session.channel_success(channel_id);
            russh_sftp::server::run(channel.into_stream(), sftp).await;
        } else {
            session.channel_failure(channel_id);
        }

        Ok(())
    }
}

#[derive(Default)]
struct SftpSession {
    version: Option<u32>,
    root_dir_read_done: bool,
    virtual_root: VirtualRoot,
    cwd_offset: PathBuf,
    handles: HashMap<String, String>,
    file_handles: HashMap<String, fs::File>,
    req_done: HashMap<u32, bool>,
    user: String,
}

impl SftpSession {
    fn new_with_username(username: String) -> Self {
        Self {
            version: None,
            root_dir_read_done: false,
            virtual_root: VirtualRoot::default(),
            cwd_offset: PathBuf::from("/"),
            handles: HashMap::new(),
            file_handles: HashMap::new(),
            req_done: HashMap::new(),
            user: username,
        }
    }

    fn check_req_done(&mut self, id: u32) -> bool {
        // match self.req_done.get(&id) {
        //     | Some(v) => *v,
        //     | None => {
        //         self.req_done.insert(id,false).unwrap();
        //         false
        //     }
        // }
        self.root_dir_read_done = !self.root_dir_read_done;
        !self.root_dir_read_done
    }
}

#[async_trait]
impl russh_sftp::server::Handler for SftpSession {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        version: u32,
        extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        if self.version.is_some() {
            error!("duplicate SSH_FXP_VERSION packet");
            return Err(StatusCode::ConnectionLost);
        }
        //偏移量默认记录为 ‘/’
        self.cwd_offset = PathBuf::from("/");

        self.version = Some(version);
        info!("version: {:?}, extensions: {:?}", self.version, extensions);
        Ok(Version::new())
    }

    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
        self.handles.remove(&_handle);

        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let mut open_options: fs::OpenOptions = pflags.into();
        if pflags.contains(OpenFlags::CREATE) {
            open_options.write(true);
        }
        let path = self.cwd_offset.join(filename);
        let path = self.virtual_root.to_real_path(&path).unwrap();
        let handle_str = format!("handle_{}", id);
        let file = open_options.open(path.clone()).unwrap();
        self.handles
            .insert(handle_str.clone(), path.to_str().unwrap().to_string());
        self.file_handles.insert(handle_str.clone(), file);
        // log example:     tracing::info!(username = "admin", action = "Open", target = "Connection", "User action logged");
        info!(username = self.user.clone(), action = "Open", target = path.to_str().unwrap(), "User action logged");
        Ok(Handle {
            id,
            handle: handle_str,
        })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(path))
            .unwrap();
        let target = real_path.clone().to_str().unwrap().to_string();
        let metadata = fs::symlink_metadata(real_path).unwrap();
        let attrs = FileAttributes::from(&metadata);
        info!(username = self.user.clone(), action = "Lstat", target = target, "User action logged");
        Ok(Attrs {
            id: id,
            attrs: attrs,
        })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
    match self.handles.get(&handle) {
        Some(vpath) => {
            let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(vpath))
            .unwrap();
            let metadata = fs::symlink_metadata(real_path.clone()).unwrap();
            let attrs = FileAttributes::from(&metadata);
            info!(username = self.user.clone(), action = "Fstat", target = real_path.to_str().unwrap(), "User action logged");
            Ok(Attrs {
                id: id,
                attrs: attrs,
            })
        }
        None => {
            Err(Self::Error::from(StatusCode::NoSuchFile))
        }
    }

    }


    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        // 状态相关的暂时不写了
        Err(self.unimplemented())
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let file = self.file_handles.get_mut(&handle).unwrap();
        let file_size = file.metadata().unwrap().len();
        if offset >= file_size {
            return Err(Self::Error::from(StatusCode::Eof));
        }
        let mut buf = vec![0; len as usize];
        file.seek(SeekFrom::Start(offset)).unwrap();

        let bytes_read = file.read(&mut buf).unwrap();

        if bytes_read < len as usize {
            buf.truncate(bytes_read);
        }
        let path = self.handles.get(&handle).unwrap();
        info!(username = self.user.clone(), action = "Read", target = path, "User action logged");
        Ok(Data { id, data: buf })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let file = match self.file_handles.get_mut(&handle) {
            Some(file) => file,
            None => return Err(Self::Error::from(StatusCode::Eof)),
        };

        // 将文件指针移动到指定的偏移量
        file.seek(SeekFrom::Start(offset)).unwrap();

        // 写入数据
        let bytes_written = file.write(&data).unwrap();

        // 检查是否所有数据都已写入
        if bytes_written < data.len() {
            return Err(Self::Error::from(StatusCode::Eof));
        }
        let path = self.handles.get(&handle).unwrap();
        info!(username = self.user.clone(), action = "Write", target = path, "User action logged");
        // 返回写入操作的状态
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        let vpath = self.cwd_offset.join(filename);
        let real_path = self.virtual_root.to_real_path(&vpath).unwrap();
        fs::remove_file(real_path.clone()).unwrap();
        info!(username = self.user.clone(), action = "Remove", target = real_path.to_str().unwrap(), "User action logged");
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        // 使用winscp打开空文件夹会出错显示返回空表 很奇怪 本来就是空的啊
        info!("opendir: {}", path);
        self.root_dir_read_done = false;
        let path = self.cwd_offset.join(path);
        let handle_str = format!("handle_{}", id);
        self.handles.insert(
            handle_str.clone(),
            path.clone().to_str().unwrap().to_string(),
        );
        let real_path = self.virtual_root.to_real_path(&path).unwrap().to_str().unwrap().to_string();
        info!(username = self.user.clone(), action = "OpenDir", target = real_path, "User action logged");
        Ok(Handle {
            id,
            handle: handle_str,
        })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        info!("readdir handle: {}", handle);
        let done = self.check_req_done(id);
        match done {
            false => {
                let vpath = self.handles.get(&handle).unwrap();
                let vpath = Path::new(vpath);
                let real_path = self.virtual_root.to_real_path(&vpath).unwrap();
                let real_path = real_path.canonicalize().unwrap();
                // 读取目录
                let entries = fs::read_dir(real_path.clone()).unwrap();
                let mut files = vec![];
                for entry in entries {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    let filename = path.file_name().unwrap().to_str().unwrap().to_string();
                    let longname = format_file_info(&path).unwrap();
                    let attrs = get_file_file_attributes(&path).unwrap();
                    files.push(File {
                        filename,
                        longname,
                        attrs,
                    });
                }
                info!(username = self.user.clone(), action = "ReadDir", target = real_path.to_str().unwrap(), "User action logged");
                Ok(Name { id, files })
            }
            true => Err(Self::Error::from(StatusCode::Eof)),
        }
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(path))
            .unwrap();
        fs::create_dir(real_path.clone()).unwrap();
        info!(username = self.user.clone(), action = "MakeDir", target = real_path.to_str().unwrap(), "User action logged");
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(path))
            .unwrap();
        fs::remove_dir(real_path.clone()).unwrap();
        info!(username = self.user.clone(), action = "RemoveDir", target = real_path.to_str().unwrap(), "User action logged");
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        info!("realpath: {}", path);
        let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(path))
            .unwrap();
        let ans = self.virtual_root.to_virtual_path(&real_path).unwrap();
        if !real_path.exists() {
            return Err(StatusCode::NoSuchFile);
        }
        let longname = format_file_info(&real_path).unwrap();
        let attrs = get_file_file_attributes(&real_path).unwrap();
        info!(username = self.user.clone(), action = "RealPath", target = real_path.to_str().unwrap(), "User action logged");
        Ok(Name {
            id,
            files: vec![File {
                filename: ans.to_str().unwrap().to_string(),
                longname: longname,
                attrs: attrs,
            }],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let real_path = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(path))
            .unwrap();
        match fs::metadata(real_path.clone()) {
            Ok(metadata) => {
                let attrs = FileAttributes::from(&metadata);
                info!(username = self.user.clone(), action = "Stat", target = real_path.to_str().unwrap(), "User action logged");
                Ok(Attrs {
                    id: id,
                    attrs: attrs,
                })
            }
            Err(_) => Err(StatusCode::NoSuchFile),
        }
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let oldpath = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(oldpath))
            .unwrap();
        let newpath = self
            .virtual_root
            .to_real_path(&self.cwd_offset.join(newpath))
            .unwrap();
        fs::rename(oldpath.clone(), newpath).unwrap();
        info!(username = self.user.clone(), action = "Rename", target = oldpath.to_str().unwrap(), "User action logged");
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".to_string(),
            language_tag: "en-US".to_string(),
        })
    }
}

// 测试
#[cfg(test)]
mod tests {
    use crate::database::MockDatabasePool;

    use super::*;
    use russh::server::Server as _;
    use tracing::{debug, error, info, trace, warn};
    use tracing_subscriber::fmt;
    use tracing_subscriber::fmt::format::FmtSpan;

    fn setup_tracing() {
        tracing_subscriber::fmt()
            .with_span_events(FmtSpan::FULL)
            .with_max_level(tracing::Level::TRACE)
            .init();
    }

    #[tokio::test]
    async fn test_ssh_server() {
        // setup_tracing();

        let _ = env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .is_test(true)
            .try_init();

        let pool = MockDatabasePool::get_pool();
        let mut server = Server::<MockDatabasePool> {
            pool: pool.clone(),
            _marker: std::marker::PhantomData,
        };

        let config = russh::server::Config {
            auth_rejection_time: Duration::from_secs(3),
            auth_rejection_time_initial: Some(Duration::from_secs(0)),
            keys: vec![KeyPair::generate_ed25519().unwrap()],
            ..Default::default()
        };

        server
            .run_on_address(Arc::new(config), ("0.0.0.0", 5023))
            .await
            .unwrap();
    }
}
