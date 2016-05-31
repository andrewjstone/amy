use std::net::{TcpListener, TcpStream};

#[derive(Debug)]
pub enum Socket {
    TcpListener(TcpListener),
    TcpStream(TcpStream)
}
