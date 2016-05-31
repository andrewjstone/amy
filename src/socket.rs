use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{RawFd, AsRawFd};


#[derive(Debug)]
pub enum Socket {
    TcpListener(TcpListener),
    TcpStream(TcpStream)
}

impl Socket {
    pub fn as_raw_fd(&self) -> RawFd {
        match *self {
            Socket::TcpListener(ref s) => s.as_raw_fd(),
            Socket::TcpStream(ref s) => s.as_raw_fd()
        }
    }
}
