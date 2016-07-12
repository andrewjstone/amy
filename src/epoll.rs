use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use nix::sys::epoll::*;
use nix::Result;
use libc;

use event::Event;
use notification::Notification;
use timer::Timer;
use timerfd::TimerFd;

static EPOLL_EVENT_SIZE: usize = 1024;

pub struct KernelPoller {
    epfd: RawFd,
    registrar: KernelRegistrar,
    events: Vec<EpollEvent>
}

impl KernelPoller {
    pub fn new() -> Result<KernelPoller> {
        let epfd = try!(epoll_create());
        let registrations = Arc::new(AtomicUsize::new(1));
        Ok(KernelPoller {
            epfd: epfd,
            registrar: KernelRegistrar::new(epfd, registrations),
            events: Vec::with_capacity(EPOLL_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> KernelRegistrar {
        self.registrar.clone()
    }

    /// Wait for epoll events. Return a list of notifications. Notifications contain user data
    /// registered with epoll_ctl which is extracted from the data member returned from epoll_wait.
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification>> {

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.events.as_mut_ptr(), self.events.capacity())
        };

        let count = try!(epoll_wait(self.epfd, dst, timeout_ms as isize));

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        Ok(self.events.iter().map(|e| {
            Notification {
                id: e.data as usize,
                event: event_from_kind(e.events),
            }
        }).collect())
    }
}

#[derive(Debug, Clone)]
pub struct KernelRegistrar {
    epfd: RawFd,
    total_registrations: Arc<AtomicUsize>
}

impl KernelRegistrar {
    fn new(epfd: RawFd, registrations: Arc<AtomicUsize>) -> KernelRegistrar {
        KernelRegistrar {
            epfd: epfd,
            total_registrations: registrations
        }
    }

    pub fn register<T: AsRawFd>(&self, sock: &T, event: Event) -> Result<usize> {
        let sock_fd = sock.as_raw_fd();
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        let info = EpollEvent {
            events: kind_from_event(event),
            data: id as u64
        };

        try!(epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, sock_fd, &info));
        Ok(id)
    }

    pub fn reregister<T: AsRawFd>(&self, id: usize, sock: &T, event: Event) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let info = EpollEvent {
            events: kind_from_event(event),
            data: id as u64
        };
        epoll_ctl(self.epfd, EpollOp::EpollCtlMod, sock_fd, &info)
    }

    pub fn deregister<T: AsRawFd>(&self, sock: T) -> Result<()> {
        // info is unused by epoll on delete operations
        let info = EpollEvent {
            events: EpollEventKind::empty(),
            data: 0
        };
        let sock_fd = sock.as_raw_fd();
        epoll_ctl(self.epfd, EpollOp::EpollCtlDel, sock_fd, &info)
    }

    pub fn set_timeout(&self, timeout: usize) -> Result<Timer> {
        self.set_timer(timeout, false)
    }

    pub fn set_interval(&self, timeout: usize) -> Result<Timer> {
        self.set_timer(timeout, true)
    }

    pub fn cancel_timeout(&self, timer: Timer) -> Result<()> {
        // It would be quite a weird situation for deregister to fail, but close to succeed.
        // We must always close the fd though so it doesn't leak.
        let fd = timer.as_raw_fd();
        let res = self.deregister(timer);
        let _ = unsafe { libc::close(fd) };
        res
    }

    fn set_timer(&self, timeout: usize, recurring: bool) -> Result<Timer> {
        let timer_fd = try!(TimerFd::new(timeout, recurring));
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        let info = EpollEvent {
            events: read_event_not_oneshot(),
            data: id as u64
        };
        let fd = timer_fd.into_raw_fd();
        match epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &info) {
            Ok(_) => Ok(Timer {id: id, fd: fd}),
            Err(e) => {
                let _ = unsafe { libc::close(fd) };
                Err(e)
            }
        }
    }
}

fn event_from_kind(kind: EpollEventKind) -> Event {
    let mut event = Event::Read;
    if kind.contains(EPOLLIN) && kind.contains(EPOLLOUT) {
        event = Event::Both;
    } else if kind.contains(EPOLLOUT) {
        event = Event::Write;
    }
    event
}

fn kind_from_event(event: Event) -> EpollEventKind {
    let mut kind = EpollEventKind::empty();
    match event {
        Event::Read => {
            kind.insert(EPOLLIN);
        },
        Event::Write => {
            kind.insert(EPOLLOUT);
        },
        Event::Both => {
            kind.insert(EPOLLIN);
            kind.insert(EPOLLOUT);
        }
    }
    // All events are edge triggered and oneshot
    kind.insert(EPOLLET);
    kind.insert(EPOLLONESHOT);
    kind
}

fn read_event_not_oneshot() -> EpollEventKind {
    let mut kind = EpollEventKind::empty();
    kind.insert(EPOLLIN);
    kind.insert(EPOLLET);
    kind
}
