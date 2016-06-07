extern crate amy;

use amy::{
    Poller,
    Socket,
    Event,
    Registration
};

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::str;
use std::io::{Read, Write};

const IP: &'static str = "127.0.0.1:10001";

#[test]
fn echo_test() {
    let mut poller = Poller::<u64>::new().unwrap();
    let registrar = poller.get_registrar();
    let listener = TcpListener::bind(IP).unwrap();
    listener.set_nonblocking(true).unwrap();

    registrar.register(Socket::TcpListener(listener),
                       Event::Read,
                       5 as u64).unwrap();

    let msg = "Hello.\n";

    let h1 = thread::spawn(move || {
        let notifications = poller.wait(5000).unwrap();
        assert_eq!(1, notifications.len());
        assert_eq!(Event::Read, notifications[0].event);
        assert_eq!(5, notifications[0].registration.user_data);

        if let Socket::TcpListener(ref listener) = notifications[0].registration.socket {
            // Accept a connection and register the socket
            let conn = listener.accept().unwrap().0;
            conn.set_nonblocking(true).unwrap();
            let registrar = poller.get_registrar();
            registrar.register(Socket::TcpStream(conn), Event::Write, 6 as u64).unwrap();

            // Wait for the socket to become writable
            let mut notifications = poller.wait(5000).unwrap();
            assert_eq!(1, notifications.len());
            let notification = notifications.pop().unwrap();
            assert_eq!(Event::Write, notification.event);
            assert_eq!(6, notification.registration.user_data);

            // This write should work fine, since we have an empty tcp send buffer and the data is
            // small. Normally we'd need to maintain a state machine and retry until the data was
            // sent.
            let Registration {mut socket, ..} = *(notification.registration);
            if let Socket::TcpStream(ref mut sock) = socket {
                assert_eq!(msg.len(), sock.write(msg.as_bytes()).unwrap());
            } else {
                assert!(false);
            }

            // Register a read event, poll for it and perform the read
            registrar.register(socket, Event::Read, 7 as u64).unwrap();
            let mut notifications = poller.wait(5000).unwrap();
            assert_eq!(1, notifications.len());
            let mut notification = notifications.pop().unwrap();
            assert_eq!(Event::Read, notification.event);
            assert_eq!(7, notification.registration.user_data);
            let mut buf = [0; 7];
            if let Socket::TcpStream(ref mut sock) = notification.registration.socket {
                sock.read_exact(&mut buf).unwrap();
                assert_eq!(msg, str::from_utf8(&buf).unwrap());
            } else {
                assert!(false);
            }

        } else {
            assert!(false);
        }

    });

    let h2 = thread::spawn(move || {
        let mut sock = TcpStream::connect(IP).unwrap();
        let mut buf = [0; 7];
        sock.read_exact(&mut buf).unwrap();
        assert_eq!(msg, str::from_utf8(&buf).unwrap());
        sock.write_all(&buf).unwrap();
    });

    h1.join().unwrap();
    h2.join().unwrap();
}
