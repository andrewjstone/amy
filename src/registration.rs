use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use socket::Socket;
use event::Event;

#[derive(Debug)]
pub struct Registration<T> {
    pub socket: Socket,
    pub event: Event,
    pub user_data: T,
}

impl<T> Registration<T> {
    pub fn new(sock: Socket, event: Event, user_data: T) -> Registration<T> {
        Registration {
            socket: sock,
            event: event,
            user_data: user_data
        }
    }
}

/// Data registered with the kernel poller
#[derive(Debug)]
pub struct UserData<T> {
    pub valid_registration: Arc<AtomicBool>,
    pub registration_ptr: *mut Registration<T>
}

// This is necessary because raw pointers are not `Send`. However, the rest of the code ensures
// safety via atomics.
unsafe impl<T> Send for UserData<T> where T: Send {}
