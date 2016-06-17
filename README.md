### Usage

Add the following to your `Cargo.toml`

```toml
[dependencies]
amy = "0.1"
```

Add this to your crate root

```rust
extern crate amy;
```

### Introduction

Amy is an opinionated library that wraps Kqueue and Epoll to provide platform independent polling
and registration abstractions useful for multithreaded asynchronous network programming. The main
goal of Amy is to allow polling and registration of non-blocking socket FDs to take place on
separate threads. Therefore, Amy is not an event loop, and is most useful when used in a
multi-threaded application.

The best way to get started writing your code with Amy is to take a look at the [Multi-threaded
example test](https://github.com/andrewjstone/amy/blob/master/tests/multithread-example.rs). It
provides the canonical usage example, although is certainly not the only way to build your system.

Amy allows attaching arbitrary data to socket file descriptors during registration, which gets
returned when an event occurs on a given file descriptor. Since these FDs and their corresponding
data structures can be registered from multiple threads, it is imperative that there is only one
owner for each FD at a time. Either the FD is registered with the kernel, and the user data hidden
behind a raw pointer, or it is owned by a boxed
[Registration](https://github.com/andrewjstone/amy/blob/master/src/registration.rs) object. (Note
that this object is not clone-able on purpose). In order to prevent aliasing of the FD and user data,
because it is stored in the kernel waiting for events and also part of the registration structure
returned to user land, we ensure that when an event notification occurs the socket is removed from
the kernel poller. To do this, we mandate the use of `ONESHOT` polling and do not allow configuration
of different polling modes. In kqueue, events registered as `ONESHOT` are automatically removed when
fired, and in epoll we do this manually with another call to
[`epoll_ctl`](http://man7.org/linux/man-pages/man2/epoll_ctl.2.html).

Another benefit of giving ownership of the socket FD to a registration object that is sendable
across threads, is that reading, writing, and serialization can be performed directly on other
threads, and never block the polling thread. In order to facilitate this mode of operation, Amy
provides helper modules to coalesce data being read off a socket and decode the data into complete
messages. The messages are accessible via an iterator to make usage easier. Currently, there exists
only one of these [helpers](https://github.com/andrewjstone/amy/blob/master/src/line_reader.rs)
(for line separated text), but more are in the works.

### How is this different from Mio

[Mio](https://github.com/carllerche/mio/) is a fantastic project from which Amy has cribbed many
ideas. However, the two are distinct in a few specific areas. Specifically, Mio provides an event
loop abstraction at its core. File descriptors are registered and polled on a single thread.
Reading from and writing to those descriptors also takes place on this thread. Mio only allows tokens
(integers) to be registered with the kernel poller as unique identifiers for a given FD, and returns
these to a `ready` callback (implemented by the user) when an FD is readable or writable. In order to
support cross thread interaction, mio provides a channel allowing data to be sent to the event
loop. This channel allows the user to send messages telling the event loop handler (implemented by
the user), to register an FD or write some data. In order to get data out of the event loop, the
handler must use another channel, or channels, which are not managed by Mio.

In contrast to this, Amy allows separation of the registration and polling functionality as
described above. However, Amy also requires a channel, or channels, managed by the user to forward
Notifications from the polling thread to other threads as necessary. Amy also allows registration of
arbitrary data and not just tokens.

At this point, it may seem like using Amy is the obvious choice, but to be fair, there are a few
things Mio does that Amy does not. Mio provides timers, works on Windows, performs no allocations,
allows choice of polling/triggering modes, and is a more mature solution used by many other Rust
projects. Mio is also a much larger, more ambitious project with many more lines of code. The choice
is not that clear cut. To help decide which one to choose for your next project, below is a list of
features and (subjective) use cases showing the reasons to choose either Mio or Amy.

Choose Mio if you:
 * Need Windows support
 * Are writing a single threaded server
 * Want the absolute fastest single-threaded event loop with no allocations
 * Don't want a separate thread for a timer
 * Want to use the canonical Rust library for Async I/O

Choose Amy if you:
 * Only need `*nix` support
 * Are writing a multi-threaded server
 * Want the simplest possible abstractions to make epoll and kqueue usable and Rusty
 * Want a small, easily auditable library, with little unsafe code
 * Are comfortable using something newer and less proven

### TODO
 * Support UDP sockets
 * More reading/decoding helpers
 * Better docs

### Limitations
 * Only works on systems that implement epoll and kqueue (Linux, BSDs, Mac OSX, etc...)
 * Doesn't work on Windows, although I believe it's possible to implement Poller and Registrar
   types on top of IOCP
