/// An interface to timerfd in linux
///
/// This interface is specific to the use of Amy and not general enough to be in its own crate.

use std::ptr;
use std::mem;
use std::os::unix::io::{IntoRawFd, RawFd};
use nix::{Error, Result};
use nix::errno::Errno;
use libc::{self, c_int, CLOCK_MONOTONIC, O_NONBLOCK, O_CLOEXEC, timespec, time_t};

static TFD_NONBLOCK: c_int = O_NONBLOCK;
static TFD_CLOEXEC: c_int = O_CLOEXEC;

mod ffi {
    use libc::{c_int, timespec};

    #[repr(C)]
    pub struct Itimerspec {
        pub it_interval: timespec,
        pub it_value: timespec
    }

    extern {
        pub fn timerfd_create(clockid: c_int, flags: c_int) -> c_int;
        pub fn timerfd_settime(fd: c_int,
                               flags: c_int,
                               new: *const Itimerspec,
                               old: *mut Itimerspec) -> c_int;
    }
}

pub struct TimerFd {
    fd: c_int
}

impl TimerFd {
    pub fn new(timeout_in_ms: usize, recurring: bool) -> Result<TimerFd> {
        let fd = unsafe { ffi::timerfd_create(CLOCK_MONOTONIC, TFD_NONBLOCK | TFD_CLOEXEC) };
        if fd < 0 {
            return Err(Error::Sys(Errno::last()));
        }
        let timer_fd = TimerFd {fd: fd};
        try!(arm_timer(fd, timeout_in_ms, recurring));

        Ok(timer_fd)
    }

}

impl Drop for TimerFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

impl IntoRawFd for TimerFd {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        // Don't run drop on self when it goes out of scope because the purpose of this function is
        // to take ownership of the fd
        mem::forget(self);
        fd
    }
}

fn arm_timer(fd: c_int, timeout: usize, recurring: bool) -> Result<()> {
    let it_value = ms_to_timespec(timeout as time_t);
    let it_interval = if recurring {
        it_value.clone()
    } else {
        timespec {
            tv_sec: 0,
            tv_nsec: 0
        }
    };
    let itimerspec = ffi::Itimerspec {
        it_interval: it_interval,
        it_value: it_value
    };

    let res = unsafe { ffi::timerfd_settime(fd, 0, &itimerspec, ptr::null_mut()) };
    if res < 0 {
        return Err(Error::Sys(Errno::last()));
    }

    Ok(())
}

fn ms_to_timespec(timeout_in_ms: time_t) -> timespec {
    timespec {
        tv_sec: timeout_in_ms / 1000,
        tv_nsec: (timeout_in_ms % 1000) * 1000 * 1000
    }
}
