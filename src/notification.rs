use event::Event;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    // The unique identifier for a given socket. File descriptors can be re-used, Ids cannot.
    pub id: usize,
    pub event: Event
}
