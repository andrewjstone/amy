#[cfg(not(feature = "no_timerfd"))]
use std::os::unix::io::IntoRawFd;
#[cfg(not(feature = "no_timerfd"))]
use timer::Timer;
#[cfg(not(feature = "no_timerfd"))]
use timerfd::TimerFd;
#[cfg(not(feature = "no_timerfd"))]
use std::collections::HashMap;

use std::os::unix::io::{RawFd, AsRawFd};
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use nix::sys::epoll::*;
use nix::sys::epoll::EpollFlags;
use nix::sys::eventfd::{eventfd, EfdFlags};
use libc;
use std::io::{Result, Error, ErrorKind};
use event::Event;
use notification::Notification;
use user_event::UserEvent;
use channel::{channel, Sender, Receiver};
use nix_err_to_io_err;

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
        let epfd = epoll_create().map_err(nix_err_to_io_err)?;
        let registrations = Arc::new(AtomicUsize::new(0));
        let mut registrar = KernelRegistrar::new(epfd, registrations);
        let (tx, rx) = channel(&mut registrar)?;
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
        let epfd = epoll_create().map_err(nix_err_to_io_err)?;
        let registrations = Arc::new(AtomicUsize::new(0));
        let mut registrar = KernelRegistrar::new(epfd, registrations);
        let (tx, rx) = channel(&mut registrar)?;
        registrar.timer_tx = Some(tx);
        Ok(KernelPoller {
            epfd: epfd,
            registrar: registrar,
            events: Vec::with_capacity(EPOLL_EVENT_SIZE),
            timer_rx: rx,
            timers: TimerHeap::new()
        })
    }

    pub fn get_registrar(&self) -> KernelRegistrar {
        self.registrar.clone()
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

        let count = epoll_wait(self.epfd, dst, timeout_ms as isize).map_err(nix_err_to_io_err)?;

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        let mut timer_rx_notification = false;
        let mut notifications = Vec::with_capacity(count);
        let mut timer_ids = Vec::new();
        for e in self.events.iter() {
            let id = e.data() as usize;
            if id == self.timer_rx.get_id() {
                timer_rx_notification = true;
            } else {
                if self.timers.contains_key(&id) {
                    timer_ids.push(id);
                }
                notifications.push(Notification {
                    id: id,
                    event: event_from_flags(e.events())
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
        let count = epoll_wait(self.epfd, dst, timeout as isize).map_err(nix_err_to_io_err)?;

        // Set the length of the vector to what was filled in by the call to epoll_wait
        unsafe { self.events.set_len(count); }

        let mut timer_rx_notification = false;
        let mut notifications = Vec::with_capacity(count);
        for e in self.events.iter() {
            let id = e.data() as usize;
            if id == self.timer_rx.get_id() {
                timer_rx_notification = true;
            } else {
                notifications.push(Notification {
                    id: id,
                    event: event_from_flags(e.events())
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
        let timer_fd = TimerFd::new(timeout, recurring).map_err(nix_err_to_io_err)?;
        let mut info = EpollEvent::new(flags_from_event(Event::Read), id as u64);
        let fd = timer_fd.into_raw_fd();
        match epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &mut info) {
            Ok(_) => Ok(Timer {fd: fd, interval: recurring}),
            Err(e) => {
                let _ = unsafe { libc::close(fd) };
                Err(nix_err_to_io_err(e))
            }
        }
    }
}

#[derive(Debug, Clone)]
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

    pub fn register<T: AsRawFd>(&self, sock: &T, event: Event) -> Result<usize> {
        let sock_fd = sock.as_raw_fd();
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        let mut info = EpollEvent::new(flags_from_event(event), id as u64);

        epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, sock_fd, &mut info).map_err(nix_err_to_io_err)?;
        Ok(id)
    }

    pub fn reregister<T: AsRawFd>(&self, id: usize, sock: &T, event: Event) -> Result<()> {
        let sock_fd = sock.as_raw_fd();
        let mut info = EpollEvent::new(flags_from_event(event), id as u64);
        Ok(epoll_ctl(self.epfd, EpollOp::EpollCtlMod, sock_fd, &mut info).map_err(nix_err_to_io_err)?)
    }

    pub fn deregister<T: AsRawFd>(&self, sock: &T) -> Result<()> {
        // info is unused by epoll on delete operations
        let mut info = EpollEvent::empty();
        let sock_fd = sock.as_raw_fd();
        Ok(epoll_ctl(self.epfd, EpollOp::EpollCtlDel, sock_fd, &mut info).map_err(nix_err_to_io_err)?)
    }

    pub fn register_user_event(&mut self) -> Result<UserEvent> {
        let fd = eventfd(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK).map_err(nix_err_to_io_err)?;
        let id = self.total_registrations.fetch_add(1, Ordering::SeqCst);
        let mut info = EpollEvent::new(flags_from_event(Event::Read), id as u64);
        match epoll_ctl(self.epfd, EpollOp::EpollCtlAdd, fd, &mut info) {
            Ok(_) => Ok(UserEvent {id: id, fd: fd}),
            Err(e) => {
                let _ = unsafe { libc::close(fd) };
                Err(nix_err_to_io_err(e))
            }
        }
    }

    pub fn deregister_user_event(&mut self, event: &UserEvent) -> Result<()> {
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

fn event_from_flags(flags: EpollFlags) -> Event {
    let mut event = Event::Read;
    if flags.contains(EpollFlags::EPOLLIN) && flags.contains(EpollFlags::EPOLLOUT) {
        event = Event::Both;
    } else if flags.contains(EpollFlags::EPOLLOUT) {
        event = Event::Write;
    }
    event
}

fn flags_from_event(event: Event) -> EpollFlags {
    let mut flags = EpollFlags::empty();
    match event {
        Event::Read => {
            flags.insert(EpollFlags::EPOLLIN);
        },
        Event::Write => {
            flags.insert(EpollFlags::EPOLLOUT);
        },
        Event::Both => {
            flags.insert(EpollFlags::EPOLLIN);
            flags.insert(EpollFlags::EPOLLOUT);
        }
    }
    // All events are edge triggered
    flags.insert(EpollFlags::EPOLLET);
    flags
}
