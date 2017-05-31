use std::os::unix::io::AsRawFd;
use std::io::Result;
use std::fmt::Debug;
use event::Event;
use channel::{channel, sync_channel, Sender, SyncSender, Receiver};

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
#[derive(Debug)]
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

    pub fn try_clone(&self) -> Result<Registrar> {
        Ok(Registrar {
            inner: self.inner.try_clone()?
        })
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
    /// Will return an error if the socket is not present in the poller when using epoll. Returns no
    /// error with kqueue.
    pub fn deregister<T: AsRawFd>(&self, sock: &T) -> Result<()> {
        self.inner.deregister(sock)
    }

    /// Set a timeout in ms that fires once
    ///
    /// Note that this timeout may be delivered late due to the time taken between the calls to
    /// `Poller::wait()` exceeding the timeout, but it will never be delivered early.
    ///
    /// Note that an error will be returned if the maximum number of file descriptors is already
    /// registered with the kernel poller.
    pub fn set_timeout(&self, timeout: usize) -> Result<usize> {
        self.inner.set_timeout(timeout)
    }

    /// Set a recurring timeout in ms
    ///
    /// A notification with the returned id will be sent at the given interval. The timeout can be
    /// cancelled with a call to `cancel_timeout()`
    ///
    /// Note that if `Poller::wait()` is not called in a loop, these timeouts as well as other
    /// notifications, will not be delivered. Timeouts may be delivered late, due to the time taken
    /// between calls to `Poller::wait()` exceeding the timeout, but will never be delivered early.
    ///
    /// Note that an error will be returned if the maximum number of file descriptors is already
    /// registered with the kernel poller.
    pub fn set_interval(&self, interval: usize) -> Result<usize> {
        self.inner.set_interval(interval)
    }

    /// Cancel a recurring timeout.
    ///
    /// Note that there may be timeouts in flight already that were not yet cancelled, so it is
    /// possible that you may receive notifications after the timeout was cancelled. This can be
    /// mitigated by keeping track of live timers and only processing timeout events for known live
    /// timers.
    ///
    /// An error will be returned if the timer is not registered with the kernel poller.
    pub fn cancel_timeout(&self, timer_id: usize) -> Result<()> {
        self.inner.cancel_timeout(timer_id)
    }

    /// Create an asynchronous mpsc channel where the Receiver is registered with the kernel poller.
    ///
    /// Each new Receiver gets registered using a user space event mechanism (either eventfd or
    /// kevent depending upon OS). When a send occurs the kernel notification mechanism (a syscall on
    /// a file descriptor) will be issued to alert the kernel poller to wakeup and issue a
    /// notification for the Receiver. However, since syscalls are expensive, an optimization is
    /// made where if the kernel poller is already set to awaken, or currently processing events, a
    /// new syscall will not be made.
    ///
    /// Standard rust mpsc channels are used internally and have non-blocking semantics. Note that
    /// the return type is different since the Receiver is being registered with the kernel poller
    /// and this can fail.
    ///
    /// When a Receiver is dropped it will become unregistered.
    pub fn channel<T: Debug>(&mut self) -> Result<(Sender<T>, Receiver<T>)> {
        channel(&mut self.inner)
    }

    /// Create a synchronous mpsc channel where the Receiver is registered with the kernel poller.
    ///
    /// Each new Receiver gets registered using a user space event mechanism (either eventfd or
    /// kevent depending upon OS). When a send occurs the kernel notification mechanism (a syscall on
    /// a file descriptor) will be issued to alert the kernel poller to wakeup and issue a
    /// notification for the Receiver. However, since syscalls are expensive, an optimization is
    /// made where if the kernel poller is already set to awaken, or currently processing events, a
    /// new syscall will not be made.
    ///
    /// Standard rust synchronous mpsc channels are used internally and block when the queue is
    /// full, as given by the bound in the construcotr. Note that the return type is different
    /// since the Receiver is being registered with the kernel poller and this can fail.
    ///
    /// When a Receiver is dropped it will become unregistered.
    pub fn sync_channel<T: Debug>(&mut self, bound: usize) -> Result<(SyncSender<T>, Receiver<T>)> {
        sync_channel(&mut self.inner, bound)
    }
}
