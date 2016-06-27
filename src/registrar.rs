use nix::Result;
use socket::Socket;
use event::Event;
use handle::Handle;

#[cfg(any(target_os = "linux", target_os = "android"))]
use epoll::KernelRegistrar;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
pub use kqueue::KernelRegistrar;

#[derive(Debug)]
/// An abstraction for registering file descriptors with a kernel poller
///
/// A Registrar is tied to a Poller of the same type, and registers Sockets and user data that will
/// then be waited on by the Poller. A Registar should only be retrieved via a call to
/// Poller::get_registrar(&self), and not created on it's own.
pub struct Registrar<T> {
    inner: KernelRegistrar<T>
}

impl<T> Registrar<T> {
    /// This method is public only so it can be used directly by the Poller. Do not Use it.
    #[doc(hidden)]
    pub fn new(inner: KernelRegistrar<T>) -> Registrar<T> {
        Registrar {
            inner: inner
        }
    }

    /// Register a socket and aribtrary user data for a given event type, with a Poller.
    ///
    /// Note that only a single type of user data can be used with a given Poller/Registrar pair.
    pub fn register(&self, sock: Socket, event: Event, user_data: T) -> Result<Handle<T>> {
        self.inner.register(sock, event, user_data)
    }

    /// Re-register a socket with a different event type given it's handle.
    ///
    /// If re_register returns None, it means that the event was triggered by the
    /// poller and a notification is in flight. If `Some(Err(_))` is returned then a call to the
    /// kernel poller failed. Otherwise the re-registration was successful and we get
    /// `Some(Ok(handle))`.
    pub fn reregister(&mut self, handle: Handle<T>, event: Event) -> Option<Result<Handle<T>>> {
        self.inner.reregister(handle, event)
    }

    /// Delete a socket from the poller.
    ///
    /// None will be returned if the socket represented by the handle was not registered.
    pub fn deregister(&mut self, handle: Handle<T>) -> Option<()> {
        self.inner.deregister(handle)
    }
}

//  We impl clone instead of deriving it because T is not cloneable. This works fine because T is
//  PhantomData in the KernelRegistrar
impl<T> Clone for Registrar<T> {
    fn clone(&self) -> Registrar<T> {
        Registrar {
            inner: self.inner.clone()
        }
    }

}
