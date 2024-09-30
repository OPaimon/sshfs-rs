# SFTP 服务器

## 概述

这是一个用 Rust 实现的 SFTP 服务器。服务器支持用户认证、授权以及文件操作的日志记录。

## 功能

- **用户管理**：注册新用户并更新他们的密码。
- **认证**：使用密码安全地认证用户。
- **授权**：检查用户对特定操作的权限。
- **日志记录**：审计日志记录文件操作。

## 安装与运行

### 依赖

确保你已经安装了 Rust 和 Cargo。如果没有，请参考 [Rust 官方网站](https://www.rust-lang.org/) 进行安装。

### 构建项目

1. 构建项目：
   ```bash
   cargo build --release
   ```

### 运行服务器

1. 运行 SFTP 服务器：
   ```bash
   cargo run -- run --port 22
   ```
   你可以通过 `--port` 参数指定服务器监听的端口，默认端口为 22。

### 用户管理

1. **注册新用户**：
   ```bash
   cargo run -- auth register <username> <password>
   ```

2. **更新用户密码**：
   ```bash
   cargo run -- auth update-password <username> <new-password> <old-password>
   ```

## 配置

### 环境变量

- `PORT`：指定服务器监听的端口，默认为 22。
- `VIRTUAL_ROOT_PATH`：指定虚拟根目录的路径。如果未设置，默认为当前目录（`.`）。如果指定的路径不存在或不是目录，服务器将无法启动。
- `DATABASE_PATH`：数据库路径，使用sqlite
## 日志记录

服务器会将所有文件操作记录到数据库中，以便进行审计和追踪。