use anyhow::{Context, Result};
use ssh2::Session;
use std::net::TcpStream;

pub fn make_ssh_session_by_password(username: &str, password: &str, addr: &str) -> Result<Session> {
    let tcp = TcpStream::connect(addr).context("Failed to connect to the server")?;
    let mut sess = Session::new().context("Failed to create a new session")?;
    sess.set_tcp_stream(tcp);
    sess.handshake().context("Failed to perform SSH handshake")?;
    sess.userauth_password(username, password).context("Failed to authenticate by password")?;
    Ok(sess)
}

#[cfg(test)]
mod tests {
    use ssh2::Session;
    use std::net::TcpStream;

    use super::make_ssh_session_by_password;

    #[test]
    fn test_auth_by_password() {
        let addr = "127.0.0.1:2002";
        let tcp = TcpStream::connect(addr).unwrap();
        
        let mut sess = Session::new().unwrap();
        sess.set_tcp_stream(tcp);
        sess.handshake().unwrap();

        sess.userauth_password("admin", "admin_password").unwrap();
        assert!(sess.authenticated());
    }

    #[test]
    fn test_make_session() {
        let addr = "127.0.0.1:2002";
        let sess = make_ssh_session_by_password("admin", "admin_password", addr).unwrap();
    }
}
