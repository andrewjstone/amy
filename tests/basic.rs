extern crate amy;

use amy::{
    Poller,
    Socket,
    Event
};

use std::net::{TcpListener, TcpStream};
use std::thread;

const IP: &'static str = "127.0.0.1:10001";

#[test]
fn register_listener_and_wait_for_connection_on_separate_threads() {
    let mut poller = Poller::<u64>::new().unwrap();
    let registrar = poller.get_registrar();
    let listener = TcpListener::bind(IP).unwrap();
    listener.set_nonblocking(true).unwrap();

    registrar.register(Socket::TcpListener(listener),
                       Event::Read,
                       5 as u64).unwrap();

    let h1 = thread::spawn(move || {
        let notifications = poller.wait(5000).unwrap();
        assert_eq!(1, notifications.len());
        assert_eq!(Event::Read, notifications[0].event);
        assert_eq!(5, notifications[0].registration.user_data);
        println!("{:?}", notifications[0]);
    });

    let h2 = thread::spawn(|| {
        TcpStream::connect(IP).unwrap();
    });

    h1.join().unwrap();
    h2.join().unwrap();
}
