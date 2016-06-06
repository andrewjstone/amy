use std::os::unix::io::RawFd;
use std::collections::HashMap;
use std::slice;
use std::mem;
use std::marker::PhantomData;
use nix::sys::event::{kqueue, kevent, KEvent, EventFilter, FilterFlag};
use nix::sys::event::{EV_DELETE, EV_ADD, EV_ONESHOT};
use libc::uintptr_t;
use nix::Result;

use socket::Socket;
use event::Event;
use notification::Notification;
use registration::Registration;

#[cfg(not(target_os = "netbsd"))]
type UserData = usize;

#[cfg(target_os = "netbsd")]
type UserData = intptr_t;

static KQUEUE_EVENT_SIZE: usize = 1024;

pub struct Poller<T> {
    kqueue: RawFd,
    registrar: Registrar<T>,
    eventlist: Vec<KEvent>,
    notifications: HashMap<RawFd, Notification<T>>,
    deletions: Vec<KEvent>
}

impl<T> Poller<T> {
    pub fn new() -> Result<Poller<T>> {
        let kq = try!(kqueue());
        Ok(Poller {
            kqueue: kq,
            registrar: Registrar::new(kq),
            eventlist: Vec::with_capacity(KQUEUE_EVENT_SIZE),
            notifications: HashMap::with_capacity(KQUEUE_EVENT_SIZE),
            deletions: Vec::with_capacity(KQUEUE_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> Registrar<T> {
        self.registrar.clone()
    }

    /// Wait for kevents and return a list of Notifications. Coalesce reads and writes for the same
    /// socket into a single notification. If only a read or a write event for a given socket is
    /// present in the eventlist, check the registration to see if there is another kevent registered
    /// and remove it if so. We do this removal to prevent aliasing a pointer to the same registration    /// structure.
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification<T>>> {

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.eventlist.as_mut_ptr(), self.eventlist.capacity())
        };

        let count = try!(kevent(self.kqueue, &[], dst, timeout_ms));

        // Set the length of the vector to the number of events that was returned by kevent
        unsafe { self.eventlist.set_len(count); }

        self.coalesce_events();
        try!(self.remove_aliased_events());

        Ok(self.notifications.drain().map(|(_, v)| v).collect())
    }

    /// Combine read and write events for the same socket into a single notification.
    fn coalesce_events(&mut self) {
        for e in self.eventlist.drain(..) {
            let registration = unsafe {
                let registration_ptr: *mut Registration<T> = mem::transmute(e.udata);
                Box::from_raw(registration_ptr)
            };

            let event = event_from_filter(e.filter);
            let new_notification = Notification {
                event: event.clone(),
                registration: registration
            };

            let mut notification = self.notifications.entry(e.ident as RawFd)
                                                     .or_insert(new_notification);
            if notification.event != event {
                notification.event = Event::Both
            }
        }
    }

    fn remove_aliased_events(&mut self) -> Result<usize>{
        self.deletions.clear();
        // TODO: Does this do an allocation or just put each collected value into the existing vec?
        // It'd be a shame to have to call self.deletions.append().
        self.deletions = self.notifications.iter().filter(|&(_, ref notification)| {
            notification.registration.event != notification.event
        }).map(|(&sock_fd, ref notification)| {
            KEvent {
                ident: sock_fd as uintptr_t,
                filter: opposite_filter_from_event(&notification.event),
                flags: EV_DELETE,
                fflags: FilterFlag::empty(),
                data: 0,
                udata: 0
            }
        }).collect();
        kevent(self.kqueue, &self.deletions, &mut[], 0)
    }

}


#[derive(Debug)]
pub struct Registrar<T> {
    kqueue: RawFd,

    // We use PhantomData here so that this type logically requires being tied to type T.
    // Since the Registrar is tied to the Poller, which stores data of type T, we want to ensure
    // that when we register data, we only register data of type T.
    // See https://doc.rust-lang.org/std/marker/struct.PhantomData.html#unused-type-parameters
    phantom: PhantomData<T>
}

impl<T> Clone for Registrar<T> {
    fn clone(&self) -> Registrar<T> {
        Registrar {
            kqueue: self.kqueue.clone(),
            phantom: PhantomData
        }
    }
}

impl<T> Registrar<T> {
    /// Explicitly not public. Registrar's are tied to Pollers and are retreived via calls to
    /// poller.get_registrar().
    fn new(kq: RawFd) -> Registrar<T> {
        Registrar {
            kqueue: kq,
            phantom: PhantomData
        }
    }

    /// Allocate a Registration containing a Socket and user data of type T on the heap.
    /// Cast the pointer to this object to a UserData so it can be placed in a KEvent and passed to
    /// the kernel with a call to `kevent`. This pointer will be returned from `kevent` when
    /// the socket is ready to be used.
    ///
    /// Note that because reads and writes are separate events, the user_data pointer will be aliased
    /// if both reads and writes are registered at the same time. In order to prevent this from
    /// being dangerous, we must ensure that the user_data is not aliased when it is returned to the
    /// caller of Poller::wait(). If both EVFILT_READ and EVFILT_WRITE are enabled on the same socket, but
    /// only one of them is triggered in the call to kevent made in Poller::wait(), then the
    /// untriggered event will be removed immediately by a call to kevent so that we don't have 2
    /// aliases to the same application state being managed independently. Note, that in the case
    /// that both events are triggered, they are simply coalesced into a single event, and are
    /// already removed from kqueue because of the use of EV_ONESHOT. Therefore this scenario is not
    /// a problem.
    ///
    pub fn register(&self, sock: Socket, event: Event, user_data: T) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let registration = Box::new(Registration::new(sock, event.clone(), user_data));

        let user_data_ptr: UserData = unsafe {
            mem::transmute(Box::into_raw(registration))
        };

        let changes = make_changelist(sock_fd, event, user_data_ptr);
        try!(kevent(self.kqueue, &changes, &mut[], 0));
        Ok(())
    }
}

fn event_from_filter(filter: EventFilter) -> Event {
    if filter == EventFilter::EVFILT_READ {
        Event::Read
    } else {
        Event::Write
    }
}

fn opposite_filter_from_event(event: &Event) -> EventFilter {
    if *event == Event::Read {
        EventFilter::EVFILT_WRITE
    } else {
        EventFilter::EVFILT_READ
    }
}

/// Each event in kqueue must have its own filter. In other words, there are seperate events for
/// reads and writes on the same socket. We create the proper number of KEvents based on the enum
/// variant in the `event` paramter.
fn make_changelist(sock_fd: RawFd, event: Event, user_data: UserData) -> Vec<KEvent> {
    let mut ev = KEvent {
        ident: sock_fd as uintptr_t,
        filter: EventFilter::EVFILT_READ,
        flags: EV_ADD | EV_ONESHOT, // Add+enable the event then remove it after triggering
        fflags: FilterFlag::empty(),
        data: 0,
        udata: user_data
    };

    match event {
        Event::Read => vec![ev],
        Event::Write => {
            ev.filter = EventFilter::EVFILT_WRITE;
            vec![ev]
        },
        Event::Both => vec![ev, KEvent { filter: EventFilter::EVFILT_WRITE, .. ev }]
    }
}
