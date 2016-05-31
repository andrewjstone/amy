use socket::Socket;

#[derive(Debug)]
pub struct Registration<T> {
    pub socket: Socket,
    pub user_data: T
}

impl<T> Registration<T> {
    pub fn new(sock: Socket, user_data: T) -> Registration<T> {
        Registration {
            socket: sock,
            user_data: user_data
        }
    }
}
