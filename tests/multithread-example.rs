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
    Event,
    Notification,
    LineReader
};

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::str;
use std::io::{ErrorKind, Read, Write};

const IP: &'static str = "127.0.0.1:10002";
const DATA: &'static str = "Hello, World!\n";

#[test]
fn primary_example() {
    let poller = Poller::new().unwrap();
    let registrar = poller.get_registrar().unwrap();
    let (worker_tx, worker_rx) = channel();
    let (client_tx, client_rx) = channel();
    let (poller_tx, poller_rx) = channel();

    // Setup a listen socket in non-blocking mode
    let listener = TcpListener::bind(IP).unwrap();
    listener.set_nonblocking(true).unwrap();

    let client_tx2 = client_tx.clone();
    let h1 = thread::spawn(move || {
        run_worker(registrar, worker_rx, listener, client_tx2, poller_tx);
    });

    let h2 = thread::spawn(move || {
        run_poller(poller, worker_tx, poller_rx, client_tx);
    });

    let h3 = thread::spawn(|| {
        run_client(client_rx);
    });

    for h in vec![h1, h2, h3] {
        h.join().unwrap();
    }
}

// Create a tcp client that writes some data and expects to receive it back.
// This client uses standard blocking sockets, and doesn't use the poller/registrar at all.
//
// This client drives the flow of the test. Lines of the client will be numbered to correspond with
// sections of the poller and worker threads to describe what is happening. This should allow
// understanding the asserts and flow of the test in context.
fn run_client(rx: Receiver<()>) {
    // 1) Connect to the non-blocking listening socket registered with the poller by the worker.
    let mut sock = TcpStream::connect(IP).unwrap();

    // Wait for the worker to signal that it accepted the connection
    let _ = rx.recv().unwrap();

    // 2 + 3) Write a single line of data. This data causes a read event in the poller, which gets
    // forwarded to the worker who will read it. The worker will then register the socket again with
    // a write event, so that it can echo the data back. This write event gets forwarded to the
    // worker and it writes the data on the socket.
    sock.write_all(DATA.as_bytes()).unwrap();

    // 4) At this point the poller has received the write event and forwarded it to the worker
    // which has written the line of data. This data is received and it is checked that it is indeed
    // an echo of the original data that was sent.
    let mut buf = vec![0; DATA.len()];
    sock.read_exact(&mut buf).unwrap();
    assert_eq!(DATA, str::from_utf8(&buf).unwrap());

    // Wait for the poller to signal that it is done
    let _ = rx.recv().unwrap();
}

/// This thread runs the poller and forwards notifications to a worker thread.
fn run_poller(mut poller: Poller,
              worker_tx: Sender<Notification>,
              rx: Receiver<()>,
              client_tx: Sender<()>) {

    // 1) Wait for a connection, and ensure we get one. We started listening in the worker thread.
    // The client has connected so we only get a single read event. Forward the notification to the
    // worker.
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Read, notification.event);
    assert_eq!(1, notification.id);

    worker_tx.send(notification).unwrap();

    // Wait for the worker to accept the socket so we don't get another notification for it
    // This is only needed to make the test deterministic
    let _ = rx.recv().unwrap();

    // 2) Wait for a read event signalling data from the client. Only one line of data from a single
    // client was sent, so there is only one read notification. There is some user data registered
    // and the notification will be forwarded to the worker.
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Read, notification.event);
    assert_eq!(2, notification.id);

    // Forward the notification to the worker
    worker_tx.send(notification).unwrap();

    // Wait for the worker to do the read
    let _ = rx.recv().unwrap();

    // 3) The worker will read the data off the socket after it receives the read notification, and
    // register a write event on the same socket. The write socket buffer is empty because it's the
    // first time writing to the stream, so a write event becomes available immediately after
    // polling, and forwarded to the worker who writes data on the socket.
    let mut notifications = poller.wait(5000).unwrap();
    assert_eq!(1, notifications.len());
    let notification = notifications.pop().unwrap();
    assert_eq!(Event::Write, notification.event);
    assert_eq!(2, notification.id);

    // Forward the notification to the worker
    worker_tx.send(notification).unwrap();

    // Wait for the worker to do the write
    let _ = rx.recv().unwrap();

    // 4) We should be done here. So poll and wait for a timeout.
    let notifications = poller.wait(1000).unwrap();
    assert_eq!(0, notifications.len());

    // Signal the client to exit
    // This will cause one more notification for the closed connection
    client_tx.send(()).unwrap();
}

// This thread registers sockets and receives notifications from the poller when they are ready
fn run_worker(registrar: Registrar,
              rx: Receiver<Notification>,
              listener: TcpListener,
              client_tx: Sender<()>,
              poller_tx: Sender<()>) {

    let listener_id = registrar.register(&listener, Event::Read).unwrap();
    // This is the first registered socket, so it's Id is 1. 0 is used by a channel internal to the poller.
    assert_eq!(1, listener_id);

    // 1) Wait for a connection from the client to be noticed by the poller against the registered
    // listening socket. Then accept the connection and register it.
    let notification = rx.recv().unwrap();
    assert_eq!(notification.event, Event::Read);
    assert_eq!(notification.id, listener_id);

    // Accept the socket and register it
    let (mut socket, _) = listener.accept().unwrap();
    socket.set_nonblocking(true).unwrap();
    let socket_id = registrar.register(&socket, Event::Read).unwrap();
    // This is the second registration of a socket, so it's Id is 2.
    assert_eq!(2, socket_id);

    // Ensure when we accept again from the listener we get an ewouldblock
    if let Err(e) = listener.accept() {
        assert_eq!(ErrorKind::WouldBlock, e.kind());
    }


    // Signal the client that the the connection notification was received
    // Note this isn't necessary in production, it's just here to make the test deterministic.
    client_tx.send(()).unwrap();

    // Signal the poller that the connection was accepted
    // This also is only here to make the test deterministic
    poller_tx.send(()).unwrap();

    // 2) Data was received on the socket from the client, the read event was handled by the poller
    // and forwarded to this worker.
    //
    // Receive notification that there is data to be read, read the data, and decode it
    // Note that it's small data that shouldn't fill our buffers and will be in one message
    let notification = rx.recv().unwrap();
    assert_eq!(notification.event, Event::Read);
    assert_eq!(notification.id, socket_id);

    let mut line_reader = LineReader::new(1024);
    let bytes_read = line_reader.read(&mut socket).unwrap();
    assert_eq!(bytes_read, DATA.len());

    // Get a complete message from the line reader
    let text = line_reader.iter_mut().next().unwrap().unwrap();
    assert_eq!(DATA.to_string(), text);

    // Signal that the data was read to the poller
    poller_tx.send(()).unwrap();

    // Re-Register the socket for writing so we can echo the data back to the client
    registrar.reregister(socket_id, &socket, Event::Write).unwrap();

    // 3) The socket was available for writing, and the notification was forwarded from the poller.
    // This worker receives the notification and proceeds to echo back the read data.
    let notification = rx.recv().unwrap();
    assert_eq!(notification.event, Event::Write);
    assert_eq!(notification.id, socket_id);

    let bytes_written = socket.write(&text.as_bytes()).unwrap();
    // Assume we have enough space in the outgoing buffer to write once
    // That's plausible in this test. Don't do this in production!
    assert_eq!(text.len(), bytes_written);

    // Signal that the data was written to the poller
    poller_tx.send(()).unwrap();

    // 4) The data was sent, and we are done here.
}
