use std::os::unix::io::RawFd;
use std::slice;
use std::mem;
use std::marker::PhantomData;
use nix::sys::epoll::*;
use nix::Result;
use nix::Errno::ENOENT;

use socket::Socket;
use event::Event;
use notification::Notification;
use registration::Registration;

static EPOLL_EVENT_SIZE: usize = 1024;

pub struct KernelPoller<T> {
    epfd: RawFd,
    registrar: Registrar<T>,
    events: Vec<EpollEvent>
}

impl<T> KernelPoller<T> {
    pub fn new() -> Result<KernelPoller<T>> {
        let epfd = try!(epoll_create());
        Ok(KernelPoller {
            epfd: epfd,
            registrar: Registrar::new(epfd),
            events: Vec::with_capacity(EPOLL_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> Registrar<T> {
        self.registrar.clone()
    }

    /// Wait for epoll events. Return a list of notifications. Notifications contain user data
    /// registered with epoll_ctl which is extracted from the data member returned from epoll_wait.
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification<T>>> {

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.events.as_mut_ptr(), self.events.capacity())
        };

        let count = try!(epoll_wait(self.epfd, dst, timeout_ms as isize));

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        Ok(self.events.iter().map(|e| {
            let registration = unsafe {
               let registration_ptr: *mut Registration<T> = mem::transmute(e.data);
               Box::from_raw(registration_ptr)
            };

            // Delete the fd from the epoll instance. Fail fast with unwrap cause something is
            // seriously wrong, if this call fails.
            self.delete_registration(registration.socket.as_raw_fd()).unwrap();

            Notification {
                event: event_from_kind(e.events),
                registration: registration
            }
        }).collect())
    }

    fn delete_registration(&self, sock: RawFd) -> Result<()> {
        // info is unused by epoll on delete operations
        let info = EpollEvent {
            events: EpollEventKind::empty(),
            data: 0
        };

        epoll_ctl(self.epfd, EpollOp::EpollCtlDel, sock, &info)
    }

    // TODO: I'd like to configure this for tests only, but it doesn't compile properly
    // #[cfg(test)]
    // This is an abstracted helper function for tests that ensures that the socket is no longer
    // registered with epoll.
    pub fn assert_fail_to_delete(&self, socket: Socket) {
        let res = self.delete_registration(socket.as_raw_fd());
        assert_eq!(ENOENT, res.unwrap_err().errno());
    }

}

#[derive(Debug)]
pub struct Registrar<T> {
    epfd: RawFd,

    // We use PhantomData here so that this type logically requires being tied to type T.
    // Since the Registrar is tied to the KernelPoller, which stores data of type T, we want to ensure
    // that when we register data, we only register data of type T.
    // See https://doc.rust-lang.org/std/marker/struct.PhantomData.html#unused-type-parameters
    phantom: PhantomData<T>
}


impl<T> Clone for Registrar<T> {
    fn clone(&self) -> Registrar<T> {
        Registrar {
            epfd: self.epfd.clone(),
            phantom: PhantomData
        }
    }
}

impl<T> Registrar<T> {
    fn new(epfd: RawFd) -> Registrar<T> {
        Registrar {
            epfd: epfd,
            phantom: PhantomData
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
    pub fn register(&self, sock: Socket, event: Event, user_data: T) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let registration = Box::new(Registration::new(sock, event.clone(), user_data));

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
