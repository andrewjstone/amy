[![Build
Status](https://travis-ci.org/andrewjstone/amy.svg?branch=master)](https://travis-ci.org/andrewjstone/amy)

[API Documentation](https://crates.fyi/crates/amy/0.3.0/)

### Usage

Add the following to your `Cargo.toml`

```toml
[dependencies]
amy = "0.3"
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

On registration of a socket, Amy will return a unique ID for that socket. This id is to
be used along with the socket when re-registering for new events. Ownership of the socket is never
transfered to either the registrar or the poller. Only the raw file descriptor is copied from the
socket when registering. On notification, the unique id and event type will be returned from the
Poller.

A benefit of decoupling the Poller and Registrar is that the Poller thread never has to perform any
serialization or reading/writing to the socket. It only has to poll for events and notify the owner
of the socket that an event is ready. This can be done via a channel or other means. Any work
needing to be done can be load balanced among threads by transfering ownership of the socket
temporarily then re-registering as necessary. An additonal benefit is that there is no need to wake
up the polling thread when registering a socket, because the polling thread isn't intended to
perform registrations.

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
Notifications from the polling thread to other threads as necessary. It's not clear whether one will
provide better performance than the other as it depends on usage patterns in a real world scenario.

The choice to use Mio or Amy is not necessarily clear, so a short list of features is drawn below,
along with some (subjective) use cases showing the reasons to choose either Mio or Amy.

Choose Mio if you:
 * Need Windows support
 * Are writing a single threaded server
 * Want to use the canonical Rust library for Async I/O

Choose Amy if you:
 * Only need `\*nix` support
 * Are writing a multi-threaded server
 * Want the simplest possible abstractions to make epoll and kqueue usable and Rusty
 * Want a small, easily auditable library, with little unsafe code
 * Are comfortable using something newer and less proven

### Limitations
 * Only works on systems that implement epoll and kqueue (Linux, BSDs, Mac OSX, etc...)
 * Doesn't work on Windows, although I believe it's possible to implement Poller and Registrar
   types on top of IOCP
