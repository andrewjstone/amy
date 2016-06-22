extern crate libc;
extern crate nix;

mod socket;
mod registration;
mod event;
mod notification;
mod line_reader;
mod frame_reader;
mod poller;
mod registrar;

pub use poller::Poller;
pub use registrar::Registrar;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod epoll;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
mod kqueue;

pub use socket::Socket;
pub use registration::Registration;
pub use event::Event;
pub use notification::Notification;
pub use line_reader::LineReader;
pub use frame_reader::FrameReader;
