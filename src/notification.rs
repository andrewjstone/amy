use event::Event;
use registration::Registration;

pub struct Notification<T> {
    event: Event,
    registration: Box<Registration<T>>
}
