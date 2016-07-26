use event::Event;

/// We don't ever want to serialize notifications and send them between remote processess since they
/// are relevant only on a single process. However, they may be part of an enum to allow a single
/// message type for use between threads and processes. That enum must be allowed to be
/// serializable. Since we want to derive Encodable/Decodable for the other variants in the enum, we
/// derive Encodable/Decodable here as well.
///
/// Ideally we'd want this variant of an enum to error out if an attempted serialization was made on
/// this variant. However we can't do this because we can't manually implement the Encodable trait
/// to always error out. The reason is that the we must return an associated Error type from the
/// generic Encoder when implementing the Encodable trait, and since we can't know what encoder all
/// clients are using, we don't know the type of error to construct.
#[derive(Debug, Clone, Eq, PartialEq, RustcEncodable, RustcDecodable)]
pub struct Notification {
    // The unique identifier for a given socket. File descriptors can be re-used, Ids cannot.
    pub id: usize,
    pub event: Event
}
