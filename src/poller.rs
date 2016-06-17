use nix::Result;

use registrar::Registrar;
use notification::Notification;
use socket::Socket;

#[cfg(any(target_os = "linux", target_os = "android"))]
use epoll::KernelPoller;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
pub use kqueue::KernelPoller;

/// A Poller is an abstraction around a kernel I/O poller. Kernel pollers are platform specific.
///
/// A Poller is tied to a Registrar of the same type. The registrar allows registering file
/// descriptors with the poller, while the poller waits for read or write events on those file
/// descriptors.
pub struct Poller<T> {
    registrar: Registrar<T>,
    inner: KernelPoller<T>
}

impl<T> Poller<T> {
    pub fn new() -> Result<Poller<T>> {
        let inner = try!(KernelPoller::new());
        Ok(Poller {
            registrar: Registrar::new(inner.get_registrar()),
            inner: inner
        })
    }

    /// Return a Registrar that can be used to register Sockets with a Poller.
    ///
    /// Registrars are cloneable and can be used on a different thread from the Poller.
    pub fn get_registrar(&self) -> Registrar<T> {
        self.registrar.clone()
    }

    /// Wait for notifications from the Poller
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification<T>>> {
        self.inner.wait(timeout_ms)
    }

    // TODO: I'd like to configure this for tests only, but it doesn't compile properly
    // #[cfg(test)]
    #[doc(hidden)]
    pub fn assert_fail_to_delete(&self, socket: Socket) {
        self.inner.assert_fail_to_delete(socket)
    }
}
