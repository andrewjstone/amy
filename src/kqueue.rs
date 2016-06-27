use std::os::unix::io::RawFd;
use std::collections::HashMap;
use std::slice;
use std::mem;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nix::sys::event::{kqueue, kevent, KEvent, EventFilter, FilterFlag};
use nix::sys::event::{EV_DELETE, EV_ADD, EV_ONESHOT, EV_RECEIPT};
use nix::Errno::ENOENT;
use libc::uintptr_t;
use nix::Result;

use socket::Socket;
use event::Event;
use notification::Notification;
use registration::{Registration, UserData};
use handle::Handle;

#[cfg(not(target_os = "netbsd"))]
type UserDataPtr = usize;

#[cfg(target_os = "netbsd")]
type UserDataPtr = intptr_t;

static KQUEUE_EVENT_SIZE: usize = 1024;

pub struct KernelPoller<T> {
    kqueue: RawFd,
    count: Arc<AtomicUsize>,
    registrar: KernelRegistrar<T>,
    eventlist: Vec<KEvent>,
    notifications: HashMap<RawFd, Notification<T>>,
    deletions: Vec<KEvent>
}

impl<T> KernelPoller<T> {
    pub fn new() -> Result<KernelPoller<T>> {
        let kq = try!(kqueue());
        let count = Arc::new(AtomicUsize::new(0));
        Ok(KernelPoller {
            kqueue: kq,
            count: count.clone(),
            registrar: KernelRegistrar::new(kq, count),
            eventlist: Vec::with_capacity(KQUEUE_EVENT_SIZE),
            notifications: HashMap::with_capacity(KQUEUE_EVENT_SIZE),
            deletions: Vec::with_capacity(KQUEUE_EVENT_SIZE)
        })
    }

    pub fn get_registrar(&self) -> KernelRegistrar<T> {
        self.registrar.clone()
    }

    // Wait for kevents and return a list of Notifications. Coalesce reads and writes for the same
    // socket into a single notification. If only a read or a write event for a given socket is
    // present in the eventlist, check the registration to see if there is another kevent registered
    // and remove it if so. We do this removal to prevent aliasing a pointer to the same registration      // structure.
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

        // Increment the poll tick count so that user_data in the registrars can be garbage
        // collected.
        self.count.fetch_add(1, Ordering::SeqCst);

        Ok(self.notifications.drain().map(|(_, v)| v).collect())
    }

    // Combine read and write events for the same socket into a single notification.
    fn coalesce_events(&mut self) {
        for e in self.eventlist.drain(..) {

            // Check if We have ownership of the registration, from the invalidation below on the
            // corresponding read or write event.
            if let Some(ref mut notification) = self.notifications.get_mut(&(e.ident as RawFd)) {
                notification.event = Event::Both;
                continue;
            }

            // At this point, we try to take ownership of the user_data/registration by trying to
            // invalidate it. If we succeed we have ownership, and save the registration in the
            // notifications map. Otherwise, the owner of the handle has taken ownership so we
            // must ensure we don't deallocate it.
            //
            // We know that the handle owner hasn't freed the memory pointed to because it wait's
            // until after the current poller tick to free the memory.
            let user_data = unsafe {
                let user_data_ptr: *mut UserData<T> = mem::transmute(e.udata);
                Box::from_raw(user_data_ptr)
            };

            // The poller invalidated the registration first. Take ownership and box it up.
            if invalidate_registration(&user_data.valid_registration) {
                let registration = unsafe { Box::from_raw(user_data.registration_ptr.clone()) };

                let event = event_from_filter(e.filter);

                let notification = Notification {
                    event: event.clone(),
                    registration: registration
                };

                self.notifications.insert(e.ident as RawFd, notification);
            } else {
                // We give up ownership of the user data, since the registration is invalid.
                // The thread owning the handle will reconstruct it and drop it properly.
                mem::forget(user_data);
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

    // TODO: I'd like to configure this for tests only, but it doesn't compile properly
    // #[cfg(test)]
    // This is an abstracted helper function for tests that ensures that the socket is no longer
    // registered with kqueue.
    pub fn assert_fail_to_delete(&self, socket: Socket) {
        let read_ev = KEvent {
            ident: socket.as_raw_fd() as uintptr_t,
            filter: EventFilter::EVFILT_READ,
            flags: EV_DELETE,
            fflags: FilterFlag::empty(),
            data: 0,
            udata: 0
        };

        let write_ev = KEvent { filter: EventFilter::EVFILT_WRITE, .. read_ev};

        let res = kevent(self.kqueue, &vec![read_ev], &mut[], 0);
        assert_eq!(ENOENT, res.unwrap_err().errno());
        let res = kevent(self.kqueue, &vec![write_ev], &mut[], 0);
        assert_eq!(ENOENT, res.unwrap_err().errno());
    }
}


#[derive(Debug)]
pub struct KernelRegistrar<T> {
    kqueue: RawFd,
    old_user_data: Vec<Box<UserData<T>>>,
    poll_counter: Arc<AtomicUsize>,
    last_poll_count: usize,

    // We use PhantomData here so that this type logically requires being tied to type T.
    // Since the KernelRegistrar is tied to the KernelPoller, which stores data of type T, we want to ensure
    // that when we register data, we only register data of type T.
    // See https://doc.rust-lang.org/std/marker/struct.PhantomData.html#unused-type-parameters
    phantom: PhantomData<T>
}

impl<T> Clone for KernelRegistrar<T> {
    fn clone(&self) -> KernelRegistrar<T> {
        KernelRegistrar {
            kqueue: self.kqueue.clone(),
            old_user_data: Vec::new(),
            poll_counter: self.poll_counter.clone(),
            last_poll_count: 0,
            phantom: PhantomData
        }
    }
}

impl<T> KernelRegistrar<T> {
    // Explicitly not public. KernelRegistrar's are tied to KernelPollers and are retreived via
    // calls to poller.get_registrar().
    fn new(kq: RawFd, poll_counter: Arc<AtomicUsize>) -> KernelRegistrar<T> {
        KernelRegistrar {
            kqueue: kq,
            old_user_data: Vec::new(),
            poll_counter: poll_counter,
            last_poll_count: 0,
            phantom: PhantomData
        }
    }

    // Allocate a Registration containing a Socket and user data of type T on the heap.
    // Cast the pointer to this object to a UserDataPtr so it can be placed in a KEvent and passed to
    // the kernel with a call to `kevent`. This pointer will be returned from `kevent` when
    // the socket is ready to be used.
    //
    // Note that because reads and writes are separate events, the user_data pointer will be aliased
    // if both reads and writes are registered at the same time. In order to prevent this from //
    // being dangerous, we must ensure that the user_data is not aliased when it is returned to the
    // caller of KernelPoller::wait(). If both EVFILT_READ and EVFILT_WRITE are enabled on the same
    // socket, but only one of them is triggered in the call to kevent made in
    // KernelPoller::wait(), then the untriggered event will be removed immediately by a call to
    // kevent so that we don't have 2 aliases to the same application state being managed
    // independently. Note, that in the case that both events are triggered, they are simply
    // coalesced into a single event, and are already removed from kqueue because of the use of
    // EV_ONESHOT. Therefore this scenario is not a problem.
    pub fn register(&self, sock: Socket, event: Event, user_data: T) -> Result<Handle<T>> {
        let sock_fd = sock.as_raw_fd();
        let registration = Box::new(Registration::new(sock, event.clone(), user_data));
        let registration_valid = Arc::new(AtomicBool::new(true));
        let registration_ptr = Box::into_raw(registration);
        let userdata = Box::new(UserData {
            valid_registration: registration_valid.clone(),
            registration_ptr: registration_ptr
        });
        let userdata_ptr = Box::into_raw(userdata);

        let udata: UserDataPtr = unsafe {
            mem::transmute(userdata_ptr.clone())
        };

        let handle = Handle::new(registration_valid, userdata_ptr);
        let changes = make_changelist(sock_fd, event, udata);
        try!(kevent(self.kqueue, &changes, &mut[], 0));
        Ok(handle)
    }

    // Re-register an existing registration with a different event type
    pub fn reregister(&mut self, handle: Handle<T>, event: Event) -> Option<Result<Handle<T>>> {
        match handle.into_user_data() {
            Some(user_data) => {
                let registration = *self.update_user_data(user_data);
                self.delete(&registration);

                // Note that this user_data is of type T and not UserData<T>. It is not the same as
                // the user_data above which is of type UserData<T>.
                let Registration {socket, user_data, ..} = registration;

                Some(self.register(socket, event, user_data))

            },
            None => None
        }
    }

    pub fn deregister(&mut self, handle: Handle<T>) -> Option<()> {
        match handle.into_user_data() {
            Some(user_data) => {
                let registration = self.update_user_data(user_data);
                self.delete(&registration);
                Some(())
            },
            None => None
        }
    }

    // Mark the user_data stale and return the registration
    fn update_user_data(&mut self, user_data: Box<UserData<T>>) -> Box<Registration<T>> {
            let registration = unsafe { Box::from_raw(user_data.registration_ptr.clone()) };
            let count = self.poll_counter.load(Ordering::SeqCst);
            if count > self.last_poll_count {
                // Drop the old user data so we don't leak memory
                mem::replace(&mut self.old_user_data, Vec::new());
                self.last_poll_count = count;
            }
            // We have ownership of the user data. We need to keep it around until the next
            // poller tick. The reason is that the poller may have already gotten the user data
            // in an event, but hasn't yet boxed it into a UserData<T> to try to invalidate
            // the registration. If we drop the user data here, we will end up with a use after
            // free. Waiting until the next poller tick ensures that we are passed the point
            // where this is possible. See `fn coalesce_events(&mut self)`
            self.old_user_data.push(user_data);
            registration
    }

    // Delete any existing registrations
    //
    // Delete's are allowed to fail here. The events may already have fired and are no
    // longer in the poller.
    fn delete(&mut self, registration: &Registration<T>) {
        let read_ev = KEvent {
            ident: registration.socket.as_raw_fd() as uintptr_t,
            filter: EventFilter::EVFILT_READ,
            flags: EV_DELETE | EV_RECEIPT,
            fflags: FilterFlag::empty(),
            data: 0,
            udata: 0
        };

        let write_ev = KEvent { filter: EventFilter::EVFILT_WRITE, .. read_ev };
        let changelist = vec![read_ev, write_ev];
        let mut eventlist = changelist.clone();
        // Ensure we don't error without attempting all changes. This way errors will be reported in
        // the eventlist and all changes will be processed. Not sure this is necessary, but it
        // doesn't hurt, except for the extra allocation.
        let _ = kevent(self.kqueue, &changelist, &mut eventlist, 0);
    }
}

/// Invalidate the registration.
///
/// Returns true on success, or false if the registration is already invalid.
fn invalidate_registration(registration_valid: &Arc<AtomicBool>) -> bool {
    registration_valid.compare_and_swap(true, false, Ordering::Relaxed)
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

// Each event in kqueue must have its own filter. In other words, there are seperate events for
// reads and writes on the same socket. We create the proper number of KEvents based on the enum
// variant in the `event` paramter.
fn make_changelist(sock_fd: RawFd, event: Event, user_data: UserDataPtr) -> Vec<KEvent> {
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
