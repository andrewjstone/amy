use event::Event;
use registration::Registration;

#[derive(Debug)]
pub struct Notification<T> {
    pub event: Event,
    pub registration: Box<Registration<T>>
}
