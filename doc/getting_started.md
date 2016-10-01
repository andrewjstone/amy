# Introduction

Amy is an opinionated library that wraps Kqueue and Epoll to provide platform independent polling
and registration abstractions useful for multithreaded asynchronous network programming. The main
goal of Amy is to allow polling and registration of non-blocking async I/O types (sockets) to take
place on separate threads. Therefore, Amy is not an event loop, and is most useful when used in a
multi-threaded application.

On registration of socket, Amy will return a unique ID. This id is to be used along with the socket
when re-registering for new events. Ownership of the socket is never transferred to either the
registrar or the poller. Only the raw file descriptor is copied from the socket when registering. On
notification, the unique id and event type (Read, Write, or Both) will be returned from the Poller.

A benefit of decoupling the Poller and Registrar is that the Poller thread never has to perform any
serialization or reading/writing to the socket. It only has to poll for events and notify the owner
of the socket that an event is ready. This can be done via a channel or other means. Any work
needing to be done can be load balanced among threads by transferring ownership of the socket
temporarily then re-registering as necessary. An additional benefit is that there is no need to wake
up the polling thread when registering a socket, because the polling thread isn't intended to
perform registrations.

Additionally Amy provides support for polling on native timers and mpsc channel receivers. This
allows a complete solution for building async network based applications.

The rest of this guide will detail the core types and concepts and show examples of how Amy should
be used.

# Poller and Registrar

There are 2 core abstractions in Amy: the
[Poller](https://github.com/andrewjstone/amy/blob/master/src/poller.rs) and the
[Registrar](https://github.com/andrewjstone/amy/blob/master/src/registrar.rs). These two
abstractions are coupled via a unique instance of the kernel polling mechanism. A poller waits for
events that are registered with a registrar. Since the registrar and poller are coupled via a kernel
mechanism, registrations for events can take place on a different thread from the poller, although
this is not required. The following example shows an instantiation of a poller on one thread and the
registering of TCP sockets to implement a server on another thread. Polling returns
[Notifications](https://github.com/andrewjstone/amy/blob/master/src/notification.rs) which contain
the unique id of a registered object and whether the object is readable, writable or both. In this
example the notifications are forwarded to the registrar thread so that they can be acted upon,
since the register thread owns the sockets.

```Rust
extern crate amy;

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::collections::HashMap;
use amy::{Poller, Event};

const IP: &'static str = "127.0.0.1:10002";

let poller = Poller::new().unwrap();

// The registrar is coupled to the specific poller instance
let registrar = poller.get_registrar();

// We need a channel to send poll events from the poller thread to the registrar/worker thread
let (tx, rx) = channel();

let handle = thread::spawn(move || {
    // We need to configure the listener in non-blocking mode or we're going to have a bad time
    let listener = TcpListener::bind(IP).unwrap();
    listener.set_nonblocking(true).unwrap();

    // register the listener for Read events and get back the unique id for the listener
    let listener_id = registrar.register(&listener, Event::Read).unwrap();

    // Store a map of connections by unique id
    let connections = HashMap::new();

    loop {
        let notification = rx.recv().unwrap();
        if notification.id == listener.id {
            // We have a new connection that we need to accept. Let's do that and make it non-blocking
           let (mut socket, _) = listener.accept().unwrap();
           socket.set_nonblocking(true).unwrap();

           // Let's register the socket for Read + Write Events
           let socket_id = registrar.register(&socket, Event::Both).unwrap();

           // Store the socket in the hashmap so we know which connection to read from or write to.
           // In practice we'd also have to maintain other state such as whether the socket is currently
           // readable or writable, what data has already been read or written, and the position in the
           // stream. Helpers will be shown in the section 'Reading from and Writing to non-blocking
           // sockets'
           connections.insert(socket_id, socket);
        } else {
            if let Some(conn) = connections.get_mut(&notification.id) {
                // Read from and/or write to the socket
            }
        }
    }
});

loop {
  // Poll for kernel events. Timeout after 5 seconds.
  let notifications = poller.wait(5000).unwrap();

  // Send notifications to the registrar thread
  for n in notifications {
    tx.send(n).unwrap();
  }
}

// Don't forget to join your threads
handle.join().unwrap();

```

# Reading from and writing to sockets
To simplify introduction of the Poller and Registrar types, reading from and writing to non-blocking
sockets was elided from the previous example. Since non-blocking sockets can return partial data in
reads, or only allow writing some of the output intended at a time, they can be difficult to work
with. For reads, the user must keep track of prior data read in a buffer and wait for the socket to
become ready again so it can read the rest of a message. During writes, the user must maintain a
cursor of the current position in the data to be written. Furthermore, writes may occur from other
user code when the socket is not ready to be written. These writes must be queued for sending.

There are, of course, a myriad of ways to handle these issues. However to simplify user experience
and implementation, Amy provides a few helper types to manage reader and writer state. Two of these
are the [FrameReader](https://github.com/andrewjstone/amy/blob/master/src/frame_reader.rs) and
[FrameWriter](https://github.com/andrewjstone/amy/blob/master/src/frame_writer.rs) types, which
allow reading and writing messages framed by 4 byte size headers. The messages themselves can be
encoded using any format, but for this example we will demonstrate reading and writing using plain
old ASCIIs data. Note that in order to be useful, the readers and writers should be bundled with the
socket in a structure that can be retrieved when a notification with the matching id arrives from
the poller.

```Rust

use amy::{FrameReader, FrameWriter, Notification, Event, Registrar};

struct Conn {
    sock: TcpStream,
    reader: FrameReader,
    writer: FrameWriter
}

// Assume only TcpStream notifications for now. Error handling is done by the (elided) caller.
fn handle_poll_notification(notification: Notification,
                            registrar: &Registrar,
                            connections: &mut HashMap<usize, Conn>) -> io::Result<()> {
   if let Some(conn) = connections.get_mut(&notification.id) {
       match notification.event {
         Event::Read => {
             // Try to read the data from the connection sock. Ignore the amount of bytes read.
             let _ = try!(conn.reader.read(&mut conn.sock));

             // Iterate through all available complete messages. Note that this iterator is mutable
             // in a non-traditional sense. It returns each complete message only once and removes it
             // from the reader.
             for msg in conn.reader.iter_mut() {
                 println!("Received a complete message: {:?}", str::from_utf8(&msg).unwrap());
             }
         },
         Event::Write => {
             // Attempt to write *all* existing data queued for writing. `None` as the second
             // parameter means no new data.
             let _ = try!(conn.writer.write(&mut conn.sock, None));
         },
         Event::Both => {
             // Do a combination of the above clauses :)
             ...
         }
      }
   }
   Ok(())
}

// Write data to some socket if possible
// This is called directly from client code and does not get called after poll notifications
fn user_write(id: usize,
              registrar: &Registrar,
              connections: &mut HashMap<usize, Conn>,
              data: Vec<u8>) -> io::Result() {
    if let Some(conn) = connections.get_mut(&notification.id) {
        try!(conn.writer.write(&mut conn.sock, data));
    }
    Ok(())
}

```

# Error handling
When a socket encounters an error, it needs to be deregistered from the event loop. Assuming an
error is returned to the caller, the caller can simply do the following, using the same connection structure
as above:

```Rust
if let Some(conn) = connections.remove(&id) {
  registrar.deregister(conn.sock);
}
```

# Timers
In most network code it's useful to be able to periodically send messages or decide when a
connection is idle. For these use cases, Amy provides support for native timers using
[TimerFd](http://man7.org/linux/man-pages/man2/timerfd_create.2.html) registered with epoll on Linux
and Android and the built-in timer support in kqueue on other systems. Note that while these native
timers are required to get started, they are somewhat heavyweight in that they require both system
calls and waking up the poller in order to be used. For most applications, only a single (or a
few) native timers should be used as scheduled ticks, with a higher level timing wheel used to
provide application timeouts. Timing wheels are not provided in Amy, as the variety is high and
application dependent. Luckily there are existing implementations out there and implementing a
simple one on your own is not hard. Example usage of native timers via Amy is below:

```Rust
use amy::Poller;

const TIMER_TIMEOUT: usize = 50; // ms
const POLL_TIMEOUT: usize = 5000; // ms

let mut poller = Poller::new().unwrap();
let registrar = poller.get_registrar();

// A single use timer
let timer = registrar.set_timeout(TIMER_TIMEOUT).unwrap();

// Wait for the single notification from the timer timeout
let notifications = poller.wait(POLL_TIMEOUT).unwrap();
assert_eq!(timer.get_id(), notifications[0].id);

// Set a recurring timer
let timer = registrar.set_interval(TIMER_TIMEOUT).unwrap();

for _ in 1..5 {
  let notifications = poller.wait(POLL_TIMEOUT).unwrap();
  // We must re-arm the timer everytime it's used. (Thanks Linux)
  // Note that this doesn't change the timing. It just ensures that a new notification occurs.
  timer.arm();
  ...
}

// Only interval timers must be cancelled if not needed anymore
registrar.cancel_timeout(timer).unwrap();
```

# Channels
Sometimes you want to use Amy in a single threaded manner. Other times you may need to notify the
polling thread of some information. For both of these cases, using a channel to receive information from
other threads may be necessary. To prevent unnecessary delays it'd be nice to wakeup the poller when
there is a message waiting to be received. For these usecases Amy provides a wrapper around mpsc
sync and async channels that automatically registers the channel receiver with the kernel poller in
order to allow information to be delivered to the poll thread. Below is a minimal example showing
async channel usage. Note that the return type of `channel()` is different in Amy than in the
standard library. This is because the call can fail, since it has to register with the kernel
poller.

For more details see the [channel test](https://github.com/andrewjstone/amy/blob/master/tests/channel_test.rs).

```Rust
let mut poller = Poller::new().unwrap();
let registrar= poller.get_registrar();
let (tx, rx) = registrar.channel().unwrap();

// Spawn a thread external to the poller to send on the channel
thread::spawn(move || {
    // Send causes the poller to wakeup
    tx.send("a").unwrap();
});

let notifications = poller.wait(5000).unwrap();
assert_eq!(rx.get_id(), notifications[0].id);
assert_eq!("a", rx.try_recv().unwrap());
```
