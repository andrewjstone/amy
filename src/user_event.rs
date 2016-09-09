use std::os::unix::io::{RawFd, AsRawFd};
use std::io::{Result, Error};

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::mem;
#[cfg(any(target_os = "linux", target_os = "android"))]
use libc;

/// An opaque handle to a user level event.
///
/// On Linux this contains a file descriptor created with
/// [eventfd()](http://man7.org/linux/man-pages/man2/eventfd.2.html)
#[cfg(any(target_os = "linux", target_os = "android"))]
#[derive(Debug, Clone)]
pub struct UserEvent {
    #[doc(hidden)]
    pub id: usize,

    #[doc(hidden)]
    pub fd: RawFd
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl UserEvent {
    pub fn get_id(&self) -> usize {
        self.id
    }

    pub fn clear(&self) -> Result<()> {
        let buf: u64 = 0;
        unsafe {
            let ptr: *mut libc::c_void = mem::transmute(&buf);
            if libc::read(self.fd, ptr, 8) < 0 {
                return Err(Error::last_os_error());
            }
            Ok(())
        }
    }

    pub fn trigger(&self) -> Result<()> {
        let buf: u64 = 1;
        unsafe {
            let ptr: *const libc::c_void = mem::transmute(&buf);
            if libc::write(self.fd, ptr, 8) < 0 {
                return Err(Error::last_os_error());
            }
            Ok(())
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
impl AsRawFd for UserEvent {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

/* NON LINUX SYSTEMS */

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub use kqueue::KernelRegistrar;

/// An opaque handle to a user level event.
///
/// On Kqueue base systems there is no file descriptor
#[cfg(not(any(target_os = "linux", target_os = "android")))]
#[derive(Debug, Clone)]
pub struct UserEvent {
    #[doc(hidden)]
    pub id: usize,

    #[doc(hidden)]
    pub registrar: KernelRegistrar
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
impl UserEvent {
    pub fn get_id(&self) -> usize {
        self.id
    }

    // Nothing to do on Kqueue based systems
    pub fn clear(&self) -> Result<()> {
        self.registrar.clear_user_event(&self).map_err(|e| Error::from(e))
    }

    pub fn trigger(&self) -> Result<()> {
        self.registrar.trigger_user_event(&self).map_err(|e| Error::from(e))
    }
}

// We don't actually need a RawFd for kqueue and don't want to shrink the id size from usize to i32
#[cfg(not(any(target_os = "linux", target_os = "android")))]
impl AsRawFd for UserEvent {
    fn as_raw_fd(&self) -> RawFd {
        0
    }
}
