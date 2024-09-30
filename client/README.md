# SFTP FUSE Mount Tool

## 项目描述
这是一个使用 Rust 编写的简单工具，用于通过 FUSE（Filesystem in Userspace）将 SFTP 文件系统挂载到本地目录。该项目允许用户通过命令行指定 SFTP 服务器的地址、用户名、密码以及挂载点，从而实现对远程 SFTP 文件系统的访问。

## 功能特性
- 支持通过命令行参数指定 SFTP 服务器的地址、用户名和密码。
- 支持自动挂载和卸载。
- 支持允许 root 用户访问挂载的文件系统。
- 使用 FUSE 实现文件系统的挂载。

## 安装与使用

### 安装依赖
确保你已经安装了 Rust 和 Cargo。然后，使用以下命令安装依赖：

```bash
cargo build --release
```

### 使用方法
运行以下命令来挂载 SFTP 文件系统：

```bash
cargo run --release -- --addr <SFTP_SERVER_ADDR> --username <USERNAME> --password <PASSWORD> <MOUNT_POINT>
```

例如：

```bash
cargo run --release -- --addr localhost:22 --username user --password pass /mnt/sftp
```

### 可选参数
- `--auto_unmount`: 在进程退出时自动卸载文件系统。
- `--allow-root`: 允许 root 用户访问挂载的文件系统。

## 示例
假设你有一个 SFTP 服务器运行在 `localhost:22`，用户名为 `user`，密码为 `pass`，你可以将它挂载到 `/mnt/sftp`：

```bash
cargo run --release -- --addr localhost:22 --username user --password pass /mnt/sftp
```

挂载成功后，你可以在 `/mnt/sftp` 目录下访问 SFTP 服务器上的文件。

## 作者
- [OPaimoe](https://github.com/OPaimon)