use std::os::unix::io::{RawFd, AsRawFd, IntoRawFd};
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use nix::sys::epoll::*;
use nix::sys::eventfd::{eventfd, EFD_CLOEXEC, EFD_NONBLOCK};
use libc;
use std::io::{Result, Error, ErrorKind};
use event::Event;
use notification::Notification;
use timer::Timer;
use timerfd::TimerFd;
use user_event::UserEvent;
use channel::{channel, Sender, Receiver};

#[cfg(feature = "no_timerfd")]
use timer_heap::{TimerHeap, TimerEntry};

static EPOLL_EVENT_SIZE: usize = 1024;

#[derive(Debug, Clone)]
pub enum TimerMsg {
    StartTimer {id: usize, timeout_ms: usize},
    StartInterval {id: usize, timeout_ms: usize},
    Cancel {id: usize}
}

pub struct KernelPoller {
    epfd: RawFd,
    registrar: KernelRegistrar,
    events: Vec<EpollEvent>,
    timer_rx: Receiver<TimerMsg>,

    #[cfg(not(feature = "no_timerfd"))]
    timers: HashMap<usize, Timer>,

    #[cfg(feature = "no_timerfd")]
    timers: TimerHeap
}

impl KernelPoller {

    #[cfg(not(feature = "no_timerfd"))]
    pub fn new() -> Result<KernelPoller> {
        let epfd = epoll_create()?;
        let registrations = Arc::new(AtomicUsize::new(0));
        let mut registrar = KernelRegistrar::new(epfd, registrations);
        let (tx, rx) = channel(registrar.try_clone()?)?;
        registrar.timer_tx = Some(tx);
        Ok(KernelPoller {
            epfd: epfd,
            registrar: registrar,
            events: Vec::with_capacity(EPOLL_EVENT_SIZE),
            timer_rx: rx,
            timers: HashMap::new()
        })
    }

    #[cfg(feature = "no_timerfd")]
    pub fn new() -> Result<KernelPoller> {
        let epfd = epoll_create()?;
        let registrations = Arc::new(AtomicUsize::new(0));
        let mut registrar = KernelRegistrar::new(epfd, registrations);
        let (tx, rx) = channel(registrar.try_clone()?)?;
        registrar.timer_tx = Some(tx);
        Ok(KernelPoller {
            epfd: epfd,
            registrar: registrar,
            events: Vec::with_capacity(EPOLL_EVENT_SIZE),
            timer_rx: rx,
            timers: TimerHeap::new()
        })
    }



    pub fn get_registrar(&self) -> Result<KernelRegistrar> {
        self.registrar.try_clone()
    }

    /// Wait for epoll events. Return a list of notifications. Notifications contain user data
    /// registered with epoll_ctl which is extracted from the data member returned from epoll_wait.
    #[cfg(not(feature = "no_timerfd"))]
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification>> {

        // We may have gotten a timer registration while awake, don't bother sleeping just to
        // immediately wake up again.
        self.receive_timer_messages()?;

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.events.as_mut_ptr(), self.events.capacity())
        };

        let count = try!(epoll_wait(self.epfd, dst, timeout_ms as isize));

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        let mut timer_rx_notification = false;
        let mut notifications = Vec::with_capacity(count);
        let mut timer_ids = Vec::new();
        for e in self.events.iter() {
            let id = e.data as usize;
            if id == self.timer_rx.get_id() {
                timer_rx_notification = true;
            } else {
                if self.timers.contains_key(&id) {
                    timer_ids.push(id);
                }
                notifications.push(Notification {
                    id: id,
                    event: event_from_kind(e.events)
                });
            }
        }
        if timer_rx_notification {
            self.receive_timer_messages()?;
        }

        self.handle_timer_notifications(timer_ids)?;

        Ok(notifications)
    }

    /// Wait for epoll events
    /// If timers are in use, the `timeout_ms` parameter may be ignored, as the epoll timeout
    /// becomes the minimum of the remaining time on the earliest timer scheduled to fire and
    /// `timeout_ms`.
    #[cfg(feature = "no_timerfd")]
    pub fn wait(&mut self, timeout_ms: usize) -> Result<Vec<Notification>> {

        // We may have gotten a timer registration while awake, don't bother sleeping just to
        // immediately wake up again.
        self.receive_timer_messages();

        // Create a buffer to read events into
        let dst = unsafe {
            slice::from_raw_parts_mut(self.events.as_mut_ptr(), self.events.capacity())
        };

        let expired = self.timers.expired();
        if !expired.is_empty() {
            return Ok(expired);
        }

        let timeout = self.timers.earliest_timeout(timeout_ms);
        let count = try!(epoll_wait(self.epfd, dst, timeout as isize));

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        let mut timer_rx_notification = false;
        let mut notifications = Vec::with_capacity(count);
        for e in self.events.iter() {
            let id = e.data as usize;
            if id == self.timer_rx.get_id() {
                timer_rx_notification = true;
            } else {
                notifications.push(Notification {
                    id: id,
                    event: event_from_kind(e.events)
                });
            }
        }
        if timer_rx_notification {
            self.receive_timer_messages();
        }

        let expired = self.timers.expired();
        notifications.extend(expired);

        Ok(notifications)
    }

    #[cfg(not(feature = "no_timerfd"))]
    fn receive_timer_messages(&mut self) -> Result<()> {
        while let Ok(msg) = self.timer_rx.try_recv() {
            match msg {
                TimerMsg::StartTimer {id, timeout_ms} => {
                    let timer = self.set_timer(id, timeout_ms, false)?;
                    self.timers.insert(id, timer);
                },
                TimerMsg::StartInterval {id, timeout_ms} => {
                    let timer = self.set_timer(id, timeout_ms, true)?;
                    self.timers.insert(id, timer);
                },
                TimerMsg::Cancel {id} => {
                    // Removing the timer from the map will cause it to be dropped, which closes its fd
                    // and subsequently removes it from epoll.
                    self.timers.remove(&id);
                }
            }
        }
        Ok(())
    }

    #[cfg(feature = "no_timerfd")]
    fn receive_timer_messages(&mut self) {
        while let Ok(msg) = self.timer_rx.try_recv() {
            match msg {
                TimerMsg::StartTimer {id, timeout_ms} => {
                    let timer = TimerEntry::new(id, timeout_ms as u64, false);
                    self.timers.insert(timer);
                },
                TimerMsg::StartInterval {id, timeout_ms} => {
                    let timer = TimerEntry::new(id, timeout_ms as u64, true);
                    self.timers.insert(timer);
                },
                TimerMsg::Cancel {id} => {
                    // Removing the timer from the map will cause it to be dropped, which closes its fd
                    // and subsequently removes it from epoll.
                    self.timers.remove(id);
                }
            }
        }
    }

    #[cfg(not(feature = "no_timerfd"))]
    fn handle_timer_notifications(&mut self, ids: Vec<usize>) -> Result<()> {
        for id in ids {
            let mut interval = false;
            if let Some(timer) = self.timers.get(&id) {
                if timer.interval {
                    interval = true;
                    timer.arm()?;
                }
            }
            if !interval {
                self.timers.remove(&id);
            }
        }
        return Ok(())
    }

    #[cfg(not(feature = "no_timerfd"))]
    fn set_timer(&self, id: usize, timeout: usize, recurring: bool) -> Result<Timer> {
        let timer_fd = try!(TimerFd::new(timeout, recurring));
        let info = EpollEvent {
            events: kind_from_event(Event::Read),
            data: id as u64
        };
        let fd = timer_fd.into_raw_fd();
        match epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &info) {
            Ok(_) => Ok(Timer {fd: fd, interval: recurring}),
            Err(e) => {
                let _ = unsafe { libc::close(fd) };
                Err(e.into())
            }
        }
    }
}

#[derive(Debug)]
pub struct KernelRegistrar {
    epfd: RawFd,
    total_registrations: Arc<AtomicUsize>,

    // This sender is strictly used to send timer registrations and cancellations to the poller
    //
    // Since initializing a channel requires an existing KernelRegistrar, we must first
    // create the channel with a registrar that has a `None` value for timer_tx. This is fine, as
    // channels have no need to create timers.
    timer_tx: Option<Sender<TimerMsg>>
}

impl KernelRegistrar {
    fn new(epfd: RawFd, registrations: Arc<AtomicUsize>) -> KernelRegistrar {
        KernelRegistrar {
            epfd: epfd,
            total_registrations: registrations,
            timer_tx: None
        }
    }

    pub fn try_clone(&self) -> Result<KernelRegistrar> {
        let timer_tx = if let Some(ref tx) = self.timer_tx {
            Some(tx.try_clone()?)
        } else {
            None
        };
        Ok(KernelRegistrar {
            epfd: self.epfd,
            total_registrations: self.total_registrations.clone(),
            timer_tx: timer_tx
        })
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
        Ok(epoll_ctl(self.epfd, EpollOp::EpollCtlMod, sock_fd, &info)?)
    }

    pub fn deregister<T: AsRawFd>(&self, sock: T) -> Result<()> {
        // info is unused by epoll on delete operations
        let info = EpollEvent {
            events: EpollEventKind::empty(),
            data: 0
        };
        let sock_fd = sock.as_raw_fd();
        Ok(epoll_ctl(self.epfd, EpollOp::EpollCtlDel, sock_fd, &info)?)
    }

    pub fn register_user_event(&mut self) -> Result<UserEvent> {
        let fd = try!(eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK));
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        let info = EpollEvent {
            events: kind_from_event(Event::Read),
            data: id as u64
        };
        match epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &info) {
            Ok(_) => Ok(UserEvent {id: id, fd: fd}),
            Err(e) => {
                let _ = unsafe { libc::close(fd) };
                Err(e.into())
            }
        }
    }

    pub fn deregister_user_event(&mut self, event: UserEvent) -> Result<()> {
        self.deregister(event)
    }

    pub fn set_timeout(&self, timeout: usize) -> Result<usize> {
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        self.timer_tx.as_ref().unwrap().send(TimerMsg::StartTimer {id: id, timeout_ms: timeout})
            .map_err(|_| Error::new(ErrorKind::BrokenPipe, "Channel receiver dropped"))?;
        Ok(id)
    }

    pub fn set_interval(&self, timeout: usize) -> Result<usize> {
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        self.timer_tx.as_ref().unwrap().send(TimerMsg::StartInterval {id: id, timeout_ms: timeout})
            .map_err(|_| Error::new(ErrorKind::BrokenPipe, "Channel receiver dropped"))?;
        Ok(id)
    }

    pub fn cancel_timeout(&self, timer_id: usize) -> Result<()> {
        self.timer_tx.as_ref().unwrap().send(TimerMsg::Cancel {id: timer_id})
            .map_err(|_| Error::new(ErrorKind::BrokenPipe, "Channel receiver dropped"))?;
        Ok(())
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
    // All events are edge triggered
    kind.insert(EPOLLET);
    kind
}
