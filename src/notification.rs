use event::Event;
use registration::Registration;

pub struct Notification<T> {
    pub event: Event,
    pub registration: Box<Registration<T>>
}
