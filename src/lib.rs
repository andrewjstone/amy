extern crate nix;

mod socket;
mod registration;
mod event;
mod notification;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod epoll;

#[cfg(any(target_os = "linux", target_os = "android"))]
pub use epoll::{Poller, Registrar};

pub use socket::Socket;
pub use registration::Registration;
pub use event::Event;
pub use notification::Notification;
