use std::sync::{mpsc, Arc};
use std::fmt::Debug;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use user_event::UserEvent;

#[cfg(any(target_os = "linux", target_os = "android"))]
use epoll::KernelRegistrar;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
pub use kqueue::KernelRegistrar;

pub fn channel<T: Debug>(registrar: &mut KernelRegistrar) -> io::Result<(Sender<T>, Receiver<T>)> {
    let (tx, rx) = mpsc::channel();
    let pending = Arc::new(AtomicUsize::new(0));
    let user_event = try!(registrar.register_user_event().map_err(|e| io::Error::from(e)));

    let dup = try!(user_event.try_clone());

    let tx = Sender {
        tx: tx,
        user_event: user_event,
        pending: pending.clone()
    };

    let rx = Receiver {
        rx: rx,
        user_event: dup,
        pending: pending
    };

    Ok((tx, rx))
}

pub fn sync_channel<T: Debug>(registrar: &mut KernelRegistrar,
                              bound: usize) -> io::Result<(SyncSender<T>, Receiver<T>)> {
    let (tx, rx) = mpsc::sync_channel(bound);
    let pending = Arc::new(AtomicUsize::new(0));
    let user_event = try!(registrar.register_user_event().map_err(|e| io::Error::from(e)));

    let dup = try!(user_event.try_clone());

    let tx = SyncSender {
        tx: tx,
        user_event: user_event,
        pending: pending.clone()
    };

    let rx = Receiver {
        rx: rx,
        user_event: dup,
        pending: pending
    };

    Ok((tx, rx))
}


#[derive(Debug)]
pub struct Sender<T: Debug> {
    tx: mpsc::Sender<T>,
    user_event: UserEvent,
    pending: Arc<AtomicUsize>
}

impl<T: Debug> Sender<T> {
    pub fn send(&self, msg: T) -> Result<(), ChannelError<T>> {
        try!(self.tx.send(msg));
        if self.pending.fetch_add(1, Ordering::SeqCst) == 0 {
            // Notify the kernel poller that a read is ready
            try!(self.user_event.trigger());
        }
        Ok(())
    }

    pub fn try_clone(&self) -> io::Result<Sender<T>> {
        Ok(Sender {
            tx: self.tx.clone(),
            user_event: self.user_event.try_clone()?,
            pending: self.pending.clone()
        })
    }

    // Return the poll id for the channel
    pub fn get_id(&self) -> usize {
        self.user_event.get_id()
    }
}

#[derive(Debug)]
pub struct SyncSender<T: Debug> {
    tx: mpsc::SyncSender<T>,
    user_event: UserEvent,
    pending: Arc<AtomicUsize>
}

impl<T: Debug> SyncSender<T> {
    pub fn send(&self, msg: T) -> Result<(), ChannelError<T>> {
        try!(self.tx.send(msg));
        if self.pending.fetch_add(1, Ordering::SeqCst) == 0 {
            // Notify the kernel poller that a read is ready
            try!(self.user_event.trigger());
        }
        Ok(())
    }

    pub fn try_send(&self, msg: T) -> Result<(), ChannelError<T>> {
        try!(self.tx.try_send(msg));
        if self.pending.fetch_add(1, Ordering::SeqCst) == 0 {
            // Notify the kernel poller that a read is ready
            try!(self.user_event.trigger());
        }
        Ok(())
    }

    pub fn try_clone(&self) -> io::Result<SyncSender<T>> {
        Ok(SyncSender {
            tx: self.tx.clone(),
            user_event: self.user_event.try_clone()?,
            pending: self.pending.clone()
        })
    }

    // Return the poll id for the channel
    pub fn get_id(&self) -> usize {
        self.user_event.get_id()
    }
}

pub struct Receiver<T: Debug> {
    rx: mpsc::Receiver<T>,
    user_event: UserEvent,
    pending: Arc<AtomicUsize>
}

impl<T: Debug> Receiver<T> {
    pub fn try_recv(&self) -> Result<T, ChannelError<T>> {
        if self.pending.load(Ordering::SeqCst) == 0 {
            // Clear the kernel event and prepare for edge triggering
            try!(self.user_event.clear());

            // Try one last check to prevent a race condition where the sender puts a value on the
            // channel and writes the event after our pending.load check, but before we did the
            // read. If we just did a read this would result in a value remaining on the channel and
            // a poller that would never wake up.
            if self.pending.load(Ordering::SeqCst) == 0 {
                return Err(ChannelError::TryRecvError(mpsc::TryRecvError::Empty));
            }
            // We still have pending events, re-activate the user event so the poller will wakeup
            try!(self.user_event.trigger());
        }

        self.pending.fetch_sub(1, Ordering::SeqCst);
        self.rx.try_recv().map_err(|e| ChannelError::from(e))
    }

    pub fn get_id(&self) -> usize {
        self.user_event.id
    }
}

#[derive(Debug)]
pub enum ChannelError<T> {
    SendError(mpsc::SendError<T>),
    TrySendError(mpsc::TrySendError<T>),
    TryRecvError(mpsc::TryRecvError),
    Io(io::Error)
}

impl<T> From<io::Error> for ChannelError<T> {
    fn from(e: io::Error) -> ChannelError<T> {
        ChannelError::Io(e)
    }
}

impl<T> From<mpsc::SendError<T>> for ChannelError<T> {
    fn from(e: mpsc::SendError<T>) -> ChannelError<T> {
        ChannelError::SendError(e)
    }
}

impl<T> From<mpsc::TrySendError<T>> for ChannelError<T> {
    fn from(e: mpsc::TrySendError<T>) -> ChannelError<T> {
        ChannelError::TrySendError(e)
    }
}

impl<T> From<mpsc::TryRecvError> for ChannelError<T> {
    fn from(e: mpsc::TryRecvError) -> ChannelError<T> {
        ChannelError::TryRecvError(e)
    }
}
