use std::os::unix::io::{RawFd, AsRawFd};

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::mem;
#[cfg(any(target_os = "linux", target_os = "android"))]
use libc;

/// An opaque handle to a kernel timer instance.
///
/// On Linux this contains a file descriptor created with
/// [timerfd_create()](http://man7.org/linux/man-pages/man2/timerfd_create.2.html)
/// On systems using kqueue, a file descriptor is not needed, so it is set to 0.
#[derive(Debug, Clone)]
pub struct Timer {
    #[doc(hidden)]
    pub id: usize,

    #[doc(hidden)]
    pub fd: RawFd
}


impl Timer {
    pub fn get_id(&self) -> usize {
        self.id
    }

    /// Re-arm a recurring timer.
    ///
    /// This method must be called when an interval timer notification is received. If not called,
    /// the next timer notification will not be received.
    ///
    /// The pattern is to store the timers in a hashmap keyed by their IDs. When a timer id is received,
    /// and the timer looked up the user should call arm().
    ///
    /// This method doesn't actually change the timing of the recurring timer. An interval timer
    /// will fire exactly at the interval specified originally. This just allows the kernel poller
    /// to received the timer event and send a notification. On epoll based systems if a timer has
    /// already fired because the timer period has elapsed, the kernel poller will be woken up
    /// immediately after this call and a notification will be sent. Since this call is a no-op on
    /// kqueue based systems, timer notifications will always be sent on time regardless of calls to
    /// arm. This should not be a problem in practice as timers should be re-armed before the next
    /// timer fires. Otherwise the timer interval is too short to be useful.
    ///
    /// Note that this method isn't strictly necessary for kqueue, but due to the semantics of epoll and
    /// the usage of timerfd, it is required that this method be called. For portability, any users
    /// should always call arm() when an interval timer notification is received.
    ///
    pub fn arm(&self) {
        self._arm();
    }

    // On Linux timers are file descriptors registered with epoll. Since we use edge triggering we
    // need to read the file descriptors to change their state. Note that even if we used level
    // triggering we'd still need to do this, but for a different reason. The timer would fire
    // indefinitely as ready in the level triggered case, rather than never firing again as in the
    // edge triggered case.
    //
    // Note that if epoll_wait on Linux returned the file descriptors that were ready we wouldn't
    // need to push this call to the user. It's a sad state of affairs on Linux I'm afraid.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn _arm(&self) {
        let buf: u64 = 0;
        unsafe {
            let ptr: *mut libc::c_void = mem::transmute(&buf);
            libc::read(self.fd, ptr, 8);
        }
    }

    #[cfg(any(target_os = "bitrig", target_os = "dragonfly",
              target_os = "freebsd", target_os = "ios", target_os = "macos",
              target_os = "netbsd", target_os = "openbsd"))]
    fn _arm(&self) {}
}

impl AsRawFd for Timer {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
