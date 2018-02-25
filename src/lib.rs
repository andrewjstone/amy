extern crate libc;
extern crate nix;

#[cfg(test)]
#[macro_use]
extern crate assert_matches;

mod event;
mod notification;
mod line_reader;
mod frame_reader;
mod frame_writer;
mod poller;
mod registrar;
mod timer;
mod user_event;
mod channel;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod epoll;
#[cfg(any(target_os = "linux", target_os = "android"))]
#[cfg(not(feature = "no_timerfd"))]
mod timerfd;

#[cfg(any(target_os = "linux", target_os = "android"))]
#[cfg(feature = "no_timerfd")]
mod timer_heap;

#[cfg(any(target_os = "bitrig", target_os = "dragonfly",
          target_os = "freebsd", target_os = "ios", target_os = "macos",
          target_os = "netbsd", target_os = "openbsd"))]
mod kqueue;

pub use poller::Poller;
pub use registrar::Registrar;
pub use event::Event;
pub use notification::Notification;
pub use line_reader::LineReader;
pub use frame_reader::FrameReader;
pub use frame_writer::FrameWriter;
pub use timer::Timer;
pub use channel::{channel, Sender, Receiver, ChannelError};

use std::io::{self, ErrorKind};
use nix::Error::Sys;

fn nix_err_to_io_err(err: nix::Error) -> io::Error {
    match err {
        Sys(errno) => {
            io::Error::from(errno)
        }
        _ => {
            io::Error::new(ErrorKind::InvalidData, err)
        }
    }
}
