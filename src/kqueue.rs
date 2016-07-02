use std::os::unix::io::RawFd;
use std::collections::HashMap;
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::os::unix::io::AsRawFd;
use nix::sys::event::{kqueue, kevent, KEvent, EventFilter, FilterFlag};
use nix::sys::event::{EV_DELETE, EV_ADD, EV_ONESHOT};
use libc::uintptr_t;
use nix::Result;

use event::Event;
use notification::Notification;

#[cfg(not(target_os = "netbsd"))]
type UserData = usize;

#[cfg(target_os = "netbsd")]
type UserData = intptr_t;

static KQUEUE_EVENT_SIZE: usize = 1024;

pub struct KernelPoller {
    kqueue: RawFd,
    registrar: KernelRegistrar,
    eventlist: Vec<KEvent>,
    notifications: HashMap<RawFd, Notification>
}

impl KernelPoller {
    pub fn new() -> Result<KernelPoller> {
        let kq = try!(kqueue());
        let registrations = Arc::new(AtomicUsize::new(0));
        Ok(KernelPoller {
            kqueue: kq,
            registrar: KernelRegistrar::new(kq, registrations),
            eventlist: Vec::with_capacity(KQUEUE_EVENT_SIZE),
            notifications: HashMap::with_capacity(KQUEUE_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> KernelRegistrar {
        self.registrar.clone()
    }

    // Wait for kevents and return a list of Notifications. Coalesce reads and writes for the same
    // socket into a single notification. If only a read or a write event for a given socket is
    // present in the eventlist, check the registration to see if there is another kevent registered
    // and remove it if so. We do this removal to prevent aliasing a pointer to the same registration      // structure.
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification>> {

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.eventlist.as_mut_ptr(), self.eventlist.capacity())
        };

        let count = try!(kevent(self.kqueue, &[], dst, timeout_ms));

        // Set the length of the vector to the number of events that was returned by kevent
        unsafe { self.eventlist.set_len(count); }

        self.coalesce_events();
        Ok(self.notifications.drain().map(|(_, v)| v).collect())
    }

    // Combine read and write events for the same socket into a single notification.
    fn coalesce_events(&mut self) {
        for e in self.eventlist.drain(..) {
            let event = event_from_filter(e.filter);
            let new_notification = Notification {
                id: e.udata as usize,
                event: event.clone()
            };

            let mut notification = self.notifications.entry(e.ident as RawFd)
                                                     .or_insert(new_notification);
            if notification.event != event {
                notification.event = Event::Both
            }
        }
    }
}


#[derive(Debug, Clone)]
pub struct KernelRegistrar {
    kqueue: RawFd,
    total_registrations: Arc<AtomicUsize>
}

impl KernelRegistrar {
    // Explicitly not public. KernelRegistrar's are tied to KernelPollers and are retreived via
    // calls to poller.get_registrar().
    fn new(kq: RawFd, registrations: Arc<AtomicUsize>) -> KernelRegistrar {
        KernelRegistrar {
            kqueue: kq,
            total_registrations: registrations
        }
    }

    pub fn register<T: AsRawFd>(&self, sock: &T, event: Event) -> Result<usize> {
        let sock_fd = sock.as_raw_fd();
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst) + 1;
        let changes = make_changelist(sock_fd, event, id as UserData);
        try!(kevent(self.kqueue, &changes, &mut[], 0));
        Ok(id)
    }

    pub fn reregister<T: AsRawFd>(&self, id: usize, sock: &T, event: Event) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let changes = make_changelist(sock_fd, event, id as UserData);
        try!(kevent(self.kqueue, &changes, &mut[], 0));
        Ok(())
    }

    pub fn deregister<T: AsRawFd>(&self, sock: T) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let mut changes = make_changelist(sock_fd, Event::Both, 0);
        for e in changes.iter_mut() {
            e.flags = EV_DELETE
        }
        // Just ignore errors because, one of the events may not be present, but the deregister
        // signature ignores that fact. At this point, ownership of the socket is taken so it's
        // irrelevant anyway.
        let _ = kevent(self.kqueue, &changes, &mut[], 0);
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

// Each event in kqueue must have its own filter. In other words, there are seperate events for
// reads and writes on the same socket. We create the proper number of KEvents based on the enum
// variant in the `event` paramter.
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
