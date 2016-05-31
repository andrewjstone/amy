#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Read,
    Write,
    Both
}
