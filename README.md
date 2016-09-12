[![Build
Status](https://travis-ci.org/andrewjstone/amy.svg?branch=master)](https://travis-ci.org/andrewjstone/amy)

[API Documentation](https://docs.rs/amy)

### Usage

Add the following to your `Cargo.toml`

```toml
[dependencies]
amy = "0.5"
```

Add this to your crate root

```rust
extern crate amy;
```

### Introduction

Amy is a Rust library supporting asynchronous I/O by abstracting over the kernel pollers
[kqueue](https://www.freebsd.org/cgi/man.cgi?query=kqueue&sektion=2) and
[epoll](http://man7.org/linux/man-pages/man7/epoll.7.html). Amy has the following goals behind it's
design:

  * Clean I/O Abstractions - Don't require users to understand anything about the internals of kernel
    pollers such as triggering modes, filters, file descriptors, etc...

  * Minimal Configuration - In line with clean abstractions, choices of polling
    modes are made inside the library in order to provide the desired semantics of the library
    without the user having to understand the low level details

  * Performance - Amy performs zero run-time allocations in the poller and registrar after initial
    startup and limits system calls via non-blocking algorithms where possible

  * Minimal implementation - Amy implements just enough code to get the job done. There isn't an
    abundance of types or traits. The code should also be readable in a linear fashion without
    jumping from file to file. This should make auditing for both correctness and performance
    easier.

  * Small and consistent API - There are only a few concepts to understand, and a few functions to
    use.

  * Reuse of Rust standard types and traits - Instead of creating a wrapper around every pollable
    I/O type such as TcpStreams, the library can use the standard library types directly. The
    only requirement is that these types implement the `AsRawFd` trait and are pollable by the
    kernel mechanism.

The best way to get started writing your code with Amy is to take a look at the [Getting Started
Guide] (https://github.com/andrewjstone/amy/blob/master/doc/getting_started.md).

### How is this different from Mio

[Mio](https://github.com/carllerche/mio/) is a fantastic project from which Amy has cribbed many
ideas. However, the two are distinct in a few specific areas. The core difference is that mio is
inherently single threaded: registrations must be made on the same thread as the poller, and the
poll loop must be woken up in order to add registrations. In contrast Amy allows registrations to be
made on a separate thread from the poller without waking it. Amy also provides a smaller code base
dedicated to async network programming. It does not allow arbitrary registration of events with the
kernel poller, although this could be easily provided. Like Mio, Amy is a building block and the
choice of whether to use one or the other is simply one of preference.

The choice to use Mio or Amy is not necessarily clear, so a short list of features is drawn below,
along with some (subjective) use cases showing the reasons to choose either Mio or Amy.

Choose Mio if you:
 * Need Windows support
 * Are writing a single threaded server
 * Want to use the canonical Rust library for Async I/O

Choose Amy if you:
 * Only need `\*nix` support
 * Are writing a multi-threaded server requiring cross thread registration
 * Want a small, easily auditable library, with little unsafe code
 * Are comfortable using something newer and less proven

### Limitations
 * Only works on systems that implement epoll and kqueue (Linux, BSDs, Mac OSX, etc...)
 * Doesn't work on Windows, although I believe it's possible to implement Poller and Registrar
   types on top of IOCP
