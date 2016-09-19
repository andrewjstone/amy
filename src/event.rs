#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Read,
    Write,
    Both
}

impl Event {
    pub fn readable(&self) -> bool {
        match *self {
            Event::Read | Event::Both => true,
            _ => false
        }
    }

    pub fn writable(&self) -> bool {
        match *self {
            Event::Write | Event::Both => true,
            _ => false
        }
    }
}
