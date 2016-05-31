use socket::Socket;

pub struct Registration<T> {
    socket: Socket,
    user_data: T
}

impl<T> Registration<T> {
    pub fn new(sock: Socket, user_data: T) -> Registration<T> {
        Registration {
            socket: sock,
            user_data: user_data
        }
    }
}
