#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Read,
    Write,
    Both
}
