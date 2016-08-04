use std::io::{Result, Error};

use registrar::Registrar;
use notification::Notification;

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
pub struct Poller {
    registrar: Registrar,
    inner: KernelPoller
}

impl Poller {
    pub fn new() -> Result<Poller> {
        let inner = try!(KernelPoller::new());
        Ok(Poller {
            registrar: Registrar::new(inner.get_registrar()),
            inner: inner
        })
    }

    /// Return a Registrar that can be used to register Sockets with a Poller.
    ///
    /// Registrars are cloneable and can be used on a different thread from the Poller.
    pub fn get_registrar(&self) -> Registrar {
        self.registrar.clone()
    }

    /// Wait for notifications from the Poller
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification>> {
        self.inner.wait(timeout_ms).map_err(|e| Error::from(e))
    }
}
