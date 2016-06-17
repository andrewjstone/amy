/// This example show the primary use case for Amy. Registering and handling events is done on a
/// seperate thread from polling for events. This prevents having to wake up the poller to register
/// a new event, as is done with typical event loops. Both epoll and kqueue support operations
/// across threads, so this wakeup strategy is unnecessary. The channel from registrar to poller
/// becomes the kernel instead of a user-space channel. When an event is ready and the poller
/// returns, the event can be sent to a separate thread/thread pool for decoding and state
/// management. When a new registration is required, that thread or another can simply register
/// again.

extern crate amy;

use amy::{
    Poller,
    Registrar,
    Socket,
    Event,
    Registration,
    Notification,
    LineReader
};

use std::net::{TcpListener, TcpStream, SocketAddr};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::str;
use std::io::{Read, Write};

const IP: &'static str = "127.0.0.1:10002";
const DATA: &'static str = "Hello, World!\n";

enum PollEvent{
    NewSock(TcpStream, SocketAddr),
    Notification(Notification<Option<LineReader>>)
}

#[test]
fn primary_example() {
    let poller = Poller::<Option<LineReader>>::new().unwrap();
    let registrar = poller.get_registrar();
    let (tx, rx) = channel();

    listen(&poller.get_registrar());

    let h1 = thread::spawn(move || {
        run_worker(registrar, rx);
    });

    let h2 = thread::spawn(move || {
        run_poller(poller, tx);
    });

    let h3 = thread::spawn(|| {
        run_client();
    });

    for h in vec![h1, h2, h3] {
        h.join().unwrap();
    }
}

/// This thread runs the poller and forwards notifications to a worker thread.
fn run_poller(mut poller: Poller<Option<LineReader>>, tx: Sender<PollEvent>) {

    // Wait for a connection, and ensure we get one. We started listening in the main thread.
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Read, notification.event);
    let registration = &notification.registration;
    assert!(registration.user_data.is_none());

    // Accept the connection and forward it to the worker
    accept(&registration.socket, &tx);

    // Wait for a read event signalling data from the client
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Read, notification.event);
    assert_eq!(true, notification.registration.user_data.is_some());

    // Forward the notification to the worker
    let msg = PollEvent::Notification(notification);
    tx.send(msg).unwrap();

    // Wait for a write event signalling a response to the client from the worker
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Write, notification.event);
    assert!(notification.registration.user_data.is_some());

    // Forward the notification to the worker
    let msg = PollEvent::Notification(notification);
    tx.send(msg).unwrap();

    // We should be done here. So poll and wait for a timeout.
    let notifications = poller.wait(1000).unwrap();
    assert_eq!(0, notifications.len());
}

/// This thread registers sockets and receives notifications from the poller when they are ready
fn run_worker(registrar: Registrar<Option<LineReader>>, rx: Receiver<PollEvent>) {
    // Receive a notification about a connection from a client, and register for read events
    if let PollEvent::NewSock(sock, _) = rx.recv().unwrap() {
        registrar.register(Socket::TcpStream(sock),
                           Event::Read,
                           Some(LineReader::new(1024))).unwrap();
    } else {
        assert!(false);
    }

    let mut outgoing = String::new();

    // Receive notification that there is data to be read, read the data, and decode it
    // Note that it's small data that shouldn't fill our buffers and will be in one message
    if let PollEvent::Notification(notification) = rx.recv().unwrap() {
        assert_eq!(notification.event, Event::Read);

        // Need to use an intermediate to destructure a box. gross.
        // see https://github.com/rust-lang/rust/issues/22205
        // and https://github.com/rust-lang/rust/issues/16223
        // TODO: Depending upon efficiency, we may want to never reallocate the registration,
        // and just use mutable refs. In this case we would create a boxed registration and pass it
        // into registrar.register() instead of the 3 parameters currently used.
        let reg = *notification.registration;
        let Registration {mut socket, mut user_data, ..} = reg;
        if let Socket::TcpStream(ref mut sock) = socket {
            let line_reader = user_data.as_mut().unwrap();
            let bytes_read = line_reader.read(sock).unwrap();
            assert_eq!(bytes_read, DATA.len());
            let text = line_reader.iter_mut().next().unwrap().unwrap();
            assert_eq!(DATA.to_string(), text);
            // Save the data to be written
            outgoing = text;
        } else {
            assert!(false);
        }

        // Register the socket for writing so we can echo the data back to the client
        // Note that we reregister the same (mutated) user_data. In some cases the buffer will be
        // partially filled and we don't want to throw that data away.
        registrar.register(socket, Event::Write, user_data).unwrap();

    } else {
        assert!(false);
    }

    // Receive notification that the socket is available for writing
    if let PollEvent::Notification(notification) = rx.recv().unwrap() {
        assert_eq!(notification.event, Event::Write);

        // Need to use an intermediate to destructure a box. gross.
        // see https://github.com/rust-lang/rust/issues/22205
        // and https://github.com/rust-lang/rust/issues/16223
        // TODO: Depending upon efficiency, we may want to never reallocate the registration,
        // and just use mutable refs. In this case we would create a boxed registration and pass it
        // into registrar.register() instead of the 3 parameters currently used.
        let reg = *notification.registration;
        let Registration {mut socket, ..} = reg;
        if let Socket::TcpStream(ref mut sock) = socket {
            let bytes_written = sock.write(&outgoing.as_bytes()).unwrap();
            // Assume we have enough space in the outgoing buffer to write once
            // That's plausible in this test. Don't do this in production!
            assert_eq!(outgoing.len(), bytes_written);
        }
    } else {
        assert!(false);
    }
}

// Create a tcp client that writes some data and expects to receive it back.
// This client uses standard blocking sockets, and doesn't use the poller/registrar at all.
fn run_client() {
    let mut sock = TcpStream::connect(IP).unwrap();
    sock.write_all(DATA.as_bytes()).unwrap();
    let mut buf = vec![0; DATA.len()];
    sock.read_exact(&mut buf).unwrap();
    assert_eq!(DATA, str::from_utf8(&buf).unwrap());
}

/// Setup a listen socket in non-blocking mode and register it for Read Events
fn listen(registrar: &Registrar<Option<LineReader>>) {
    let listener = TcpListener::bind(IP).unwrap();
    listener.set_nonblocking(true).unwrap();
    registrar.register(Socket::TcpListener(listener), Event::Read, None).unwrap();
}

/// Accept a connection, and forward it to the thread handling registrations of actual data
/// reads/writes.
fn accept(socket: &Socket, tx: &Sender<PollEvent>) {
    if let &Socket::TcpListener(ref listener) = socket {
        let conn = listener.accept().unwrap();
        let msg = PollEvent::NewSock(conn.0, conn.1);
        tx.send(msg).unwrap();
    } else {
        assert!(false);
    }
}
