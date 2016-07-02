use std::os::unix::io::AsRawFd;
use nix::Result;
use event::Event;

#[cfg(any(target_os = "linux", target_os = "android"))]
use epoll::KernelRegistrar;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
pub use kqueue::KernelRegistrar;

/// An abstraction for registering file descriptors with a kernel poller
///
/// A Registrar is tied to a Poller of the same type, and registers sockets and unique IDs for those
/// sockets as userdata that can be waited on by the poller. A Registar should only be retrieved via
/// a call to Poller::get_registrar(&self), and not created on it's own.
#[derive(Debug, Clone)]
pub struct Registrar {
    inner: KernelRegistrar
}

impl Registrar {
    /// This method is public only so it can be used directly by the Poller. Do not Use it.
    #[doc(hidden)]
    pub fn new(inner: KernelRegistrar) -> Registrar {
        Registrar {
            inner: inner
        }
    }

    /// Register a socket for a given event type, with a Poller and return it's unique ID
    ///
    /// Note that if the sock type is not pollable, then an error will be returned.
    pub fn register<T: AsRawFd>(&self, sock: &T, event: Event) -> Result<usize> {
        self.inner.register(sock, event)
    }

    /// Reregister a socket with a Poller
    pub fn reregister<T: AsRawFd>(&self, id: usize, sock: &T, event: Event) -> Result<()> {
        self.inner.reregister(id, sock, event)
    }

    /// Remove a socket from a Poller
    ///
    /// Note that ownership of the socket is taken here. Sockets should only be deregistered when
    /// the caller is done with them.
    ///
    /// Will return an error if the socket is not present in the poller when using epoll. Returns no
    /// error with kqueue.
    pub fn deregister<T: AsRawFd>(&self, sock: T) -> Result<()> {
        self.inner.deregister(sock)
    }
}
