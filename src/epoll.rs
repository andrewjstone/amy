use std::os::unix::io::RawFd;
use std::slice;
use std::mem;
use nix::sys::epoll::*;
use nix::Result;

use socket::Socket;
use event::Event;
use notification::Notification;
use registration::Registration;

static EPOLL_EVENT_SIZE: usize = 1024;

pub struct Poller {
    epfd: RawFd,
    registrar: Registrar,
    events: Vec<EpollEvent>
}

impl Poller {
    pub fn new() -> Result<Poller> {
        let epfd = try!(epoll_create());
        Ok(Poller {
            epfd: epfd,
            registrar: Registrar::new(epfd),
            events: Vec::with_capacity(EPOLL_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> Registrar {
        self.registrar.clone()
    }

    /// Wait for epoll events. Return a list of notifications. Notifications contain user data
    /// registered with epoll_ctl which is extracted from the data member returned from epoll_wait.
    pub fn wait<T>(&mut self, timeout_ms: isize) -> Result<Vec<Notification<T>>> {

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.events.as_mut_ptr(), self.events.capacity())
        };

        let count = try!(epoll_wait(self.epfd, dst, timeout_ms));

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        Ok(self.events.iter().map(|e| {
            let registration = unsafe {
               let registration_ptr: *mut Registration<T> = mem::transmute(e.data);
               Box::from_raw(registration_ptr)
            };

            Notification {
                event: event_from_kind(e.events),
                registration: registration
            }
        }).collect())
    }

}

#[derive(Debug, Clone)]
pub struct Registrar {
    epfd: RawFd
}

impl Registrar {
    fn new(epfd: RawFd) -> Registrar {
        Registrar {
            epfd: epfd
        }
    }

    /// Allocate a Registration containing a Socket and user data of type T on the heap.
    /// Cast the pointer to this object to a u64 so it can be placed in an EpollEvent and passed to
    /// the kernel with a call to `epoll_ctl`. This pointer will be returned from `epoll_wait` when
    /// the socket is ready to be used.
    ///
    /// Passing the relevant user data into the kernel allows decoupling the registration of sockets
    /// from waiting for their readyness. This means that we can register sockets on one thread and
    /// wait for them to be ready on another thread, using the kernel as the method of
    /// communication.
    ///
    /// NOTE: THIS ONLY WORKS ON 64-BIT ARCHITECTURES
    ///
    pub fn register<T>(&mut self, sock: Socket, event: Event, user_data: T) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let registration = Box::new(Registration::new(sock, user_data));

        let registration_ptr: u64 = unsafe {
            mem::transmute(Box::into_raw(registration))
        };

        let info = EpollEvent {
            events: kind_from_event(event),
            data: registration_ptr
        };

        epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, sock_fd, &info)
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
