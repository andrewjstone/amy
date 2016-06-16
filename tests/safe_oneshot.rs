/// This test ensures that when an notification is received from the poller that there are no other
/// registrations for the same socket available in the poller. In other words, there is no aliasing
/// of a raw pointer.

extern crate amy;

use amy::{
    Poller,
    Socket,
    Event
};

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::io::Write;

const IP: &'static str = "127.0.0.1:10003";

#[test]
fn alias_test() {

    // A channel between the polling thread and the client thread that instructs the client to
    // proceed with the next operation
    let (tx, rx) = channel();

    let h1 = thread::spawn(move || {
        run_poll_thread(tx);
    });

    let h2 = thread::spawn(move || {
        run_client_thread(rx);
    });

    h1.join().unwrap();
    h2.join().unwrap();

}

fn run_poll_thread(tx: Sender<()>) {
    let mut poller = Poller::<u64>::new().unwrap();
    let listener = TcpListener::bind(IP).unwrap();
    let registrar = poller.get_registrar();
    listener.set_nonblocking(true).unwrap();
    registrar.register(Socket::TcpListener(listener), Event::Read, 5 as u64).unwrap();

    // We are listening. Tell the client to connect
    tx.send(()).unwrap();

    let notifications = poller.wait(1000).unwrap();
    assert_eq!(1, notifications.len());
    assert_eq!(Event::Read, notifications[0].event);

    if let Socket::TcpListener(ref listener) = notifications[0].registration.socket {
        // Accept the connection
        let conn = listener.accept().unwrap().0;
        conn.set_nonblocking(true).unwrap();

        // Register for both write and read events. Only a write event should be available, since the
        // client has not written any data yet.
        registrar.register(Socket::TcpStream(conn), Event::Both, 6 as u64).unwrap();

        // Poll for the write event. Show that we were registered for both reads and writes though.
        let mut notifications = poller.wait(1000).unwrap();
        assert_eq!(1, notifications.len());
        let notification = notifications.pop().unwrap();
        assert_eq!(Event::Write, notification.event);
        assert_eq!(Event::Both, notification.registration.event);

        let socket = notification.registration.socket;

        // Tell the client to send some data
        tx.send(()).unwrap();

        // Ensure we timeout, since we shouldn't be registered for any events
        let notifications = poller.wait(1000).unwrap();
        assert_eq!(0, notifications.len());

        // Attempt to delete the event. This should fail since it's not registered
        poller.assert_fail_to_delete(socket);
    } else {
        assert!(false);
    }

}

fn run_client_thread(rx: Receiver<()>) {
    // Wait for the server to be listening
    rx.recv().unwrap();
    let mut sock = TcpStream::connect(IP).unwrap();

    // Wait to send data
    rx.recv().unwrap();

    sock.write_all("hello".as_bytes()).unwrap();
}
