use std::io::{self, Write};
use std::collections::LinkedList as List;
use std::mem;

/// Abstraction for writing frame buffered data to non-blocking sockets.
///
/// Note that an individual frame writer should only write to one socket, since it tracks whether or
/// not the socket is writable directly.
pub struct FrameWriter {
    is_empty: bool,
    is_writable: bool,
    current: Vec<u8>,
    written: usize,
    pending: List<Vec<u8>>
}

impl FrameWriter {
    pub fn new() -> FrameWriter {
        FrameWriter {
            is_empty: true,
            is_writable: true,
            current: Vec::new(),
            written: 0,
            pending: List::new()
        }
    }

    /// Write as much possible stored data to the writer, along with optional new data.
    ///
    /// If data is not `None`, compute it's header and put it on the front of the pending list
    /// followed by the data, as long as the writer isn't empty. If the writer is empty, compute the
    /// header, put it in current and push the data onto the pending list.
    ///
    /// Returns `Ok(true)` if the socket is still writable, and `Ok(false)` if it's not writable and
    /// needs to be re-registered. Returns any io::Error except for `EAGAIN` or `EWOULDBLOCK` which
    /// results in `OK(false)`.
    pub fn write<T: Write>(&mut self, writer: &mut T, data: Option<Vec<u8>>) -> io::Result<bool> {
        if let Some(frame) = data {
            self.append_frame(frame);
        }
        if self.is_empty {
            return Ok(self.is_writable);
        }
        if !self.is_writable {
            return Ok(false);
        }
        self.write_as_much_as_possible(writer)
    }

    /// Tell the frame writer that the corresponding writer is writable again.
    pub fn writable(&mut self) {
        self.is_writable = true;
    }

    pub fn is_writable(&self) -> bool {
        self.is_writable
    }

    pub fn is_empty(&self) -> bool {
        self.is_empty
    }

    fn append_frame(&mut self, frame: Vec<u8>) {
        let header = u32_to_vec(frame.len() as u32);
        if self.is_empty {
            self.current = header;
            self.pending.push_back(frame);
            self.is_empty = false;
        } else {
            self.pending.push_back(header);
            self.pending.push_back(frame);
        }
    }

    fn write_as_much_as_possible<T: Write>(&mut self, writer: &mut T) -> io::Result<bool> {
        loop {
            match writer.write(&self.current[self.written..]) {
                Ok(0) => {
                    self.is_writable = false;
                    return Ok(false);
                },
                Ok(n) => {
                    self.written += n;
                    if self.written == self.current.len() {
                        match self.pending.pop_front() {
                            None => {
                                self.written = 0;
                                self.current = Vec::new();
                                self.is_empty = true;
                                return Ok(true);
                            },
                            Some(data) => {
                                self.written = 0;
                                self.current = data;
                            }
                        }
                    }
                },
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        self.is_writable = false;
                        return Ok(false);
                    }
                    return Err(e);
                }
            }
        }
    }

}

/// Convert a u32 in native order to a 4 byte vec in big endian
pub fn u32_to_vec(n: u32) -> Vec<u8> {
    unsafe {
        let bytes: [u8; 4] = mem::transmute(n.to_be());
        bytes.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use super::FrameWriter;

    #[test]
    fn call_write_on_empty_frame_writer() {
        let mut frame_writer = FrameWriter::new();
        let mut buf = vec![0; 10];
        assert_eq!(true, frame_writer.write(&mut buf, None).unwrap());
        assert_eq!(true, frame_writer.is_empty);
    }

    #[test]
    fn call_write_on_empty_frame_writer_but_fill_writer_exactly() {
        let mut frame_writer = FrameWriter::new();
        // Leave space for the 4 byte header
        let mut buf = vec![0; 14];
        // We use a cursor wrapped around a slice instead of a vec because we want a fixed buffer
        // size. If we used a vec writes would always succeed since the vec would grow.
        let mut writer = Cursor::new(&mut buf[..]);
        let frame = vec![0; 10];
        assert_eq!(true, frame_writer.write(&mut writer, Some(frame)).unwrap());
        assert_eq!(true, frame_writer.is_empty);
        assert_eq!(false, frame_writer.write(&mut writer, Some(vec![0;1])).unwrap());
    }

    #[test]
    fn write_until_full_reset_and_write_some_more() {
        let mut frame_writer = FrameWriter::new();
        let mut buf = vec![0; 14];
        let mut writer = Cursor::new(&mut buf[..]);
        let frame = vec![0; 11];
        assert_eq!(false, frame_writer.write(&mut writer, Some(frame)).unwrap());
        assert_eq!(false, frame_writer.is_empty);
        // At this point there is 1 more byte to be written stored in the frame writer
        assert_eq!(10, frame_writer.written);
        assert_eq!(true, frame_writer.pending.is_empty());

        // Try to write the last byte, but the buffer is full
        assert_eq!(false, frame_writer.write(&mut writer, None).unwrap());

        // Make the buffer writable and the buffer size 14 bytes again.
        frame_writer.writable();
        writer.set_position(0);
        assert_eq!(true, frame_writer.is_writable);
        // Write the last byte remaining, plus a new 9 byte frame and it's 4 byte header.
        assert_eq!(true, frame_writer.write(&mut writer, Some(vec![0;9])).unwrap());
        // Ensure that the frame writer was reset because there is no more data to write
        assert_eq!(true, frame_writer.is_empty);
        assert_eq!(0, frame_writer.written);
        assert_eq!(0, frame_writer.current.len());
    }
}


