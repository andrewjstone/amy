/// Ensure all sockets operate on edge trigger mode.

extern crate amy;

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::str;
use std::io::{Read, Write};

use amy::{
    Poller,
    Event,
};

const IP: &'static str = "127.0.0.1:10008";

/// This test ensures that only one write event is received, even if no data is written. On a level
/// triggered system, write events would come on every poll.
#[test]
fn edge_trigger() {

    // Spawn a listening thread and accept one connection
    thread::spawn(|| {
        let listener = TcpListener::bind(IP).unwrap();
        let (mut sock, _) = listener.accept().unwrap();
        // When the test completes, the client will send a "stop" message to shutdown the server.
        let mut buf = String::new();
        sock.read_to_string(&mut buf).unwrap();
    });

    // Setup a client socket in non-blocking mode
    // Loop until we connect because the listener needs to start first
    let mut sock;
    loop {
      if let Ok(s) = TcpStream::connect(IP) {
          sock = s;
          break;
      }
    }
    sock.set_nonblocking(true).unwrap();

    // Create the poller and registrar
    let mut poller = Poller::new().unwrap();
    let registrar = poller.get_registrar();

    // The socket should become writable once it's connected
    let id = registrar.register(&sock, Event::Write).unwrap();
    let notifications = poller.wait(250).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(id, notifications[0].id);
    assert_eq!(Event::Write, notifications[0].event);

    // Poll as second time. There should be no notification, since the socket is edge triggered.
    let notifications = poller.wait(250).unwrap();
    assert_eq!(0, notifications.len());

    // Tell the listening thread to stop itself
    sock.write("stop".as_bytes()).unwrap();
}
