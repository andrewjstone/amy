use std::os::unix::io::RawFd;
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::os::unix::io::AsRawFd;
use nix::sys::epoll::*;
use nix::Result;

use event::Event;
use notification::Notification;

static EPOLL_EVENT_SIZE: usize = 1024;

pub struct KernelPoller {
    epfd: RawFd,
    registrar: KernelRegistrar,
    events: Vec<EpollEvent>
}

impl KernelPoller {
    pub fn new() -> Result<KernelPoller> {
        let epfd = try!(epoll_create());
        let registrations = Arc::new(AtomicUsize::new(0));
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
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst) + 1;
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
