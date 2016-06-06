use socket::Socket;
use event::Event;

#[derive(Debug)]
pub struct Registration<T> {
    pub socket: Socket,
    pub event: Event,
    pub user_data: T
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
