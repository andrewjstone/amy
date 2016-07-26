#[derive(Debug, Clone, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub enum Event {
    Read,
    Write,
    Both
}
