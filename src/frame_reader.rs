//! This reader composes frames of bytes started with a 4 byte frame header indicating the size of
//! the buffer. An exact size buffer will be allocated once the 4 byte frame header is received.

use std::io::{self, Read, Error, ErrorKind};
use std::collections::VecDeque;
use std::mem;

#[derive(Debug)]
pub struct FrameReader {
    frames: Frames
}

impl FrameReader {
    pub fn new(max_frame_size: u32) -> FrameReader {
        FrameReader {
            frames: Frames::new(max_frame_size)
        }
    }

    pub fn read<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        self.frames.read(reader)
    }

    pub fn iter_mut(&mut self) -> Iter {
        Iter {
            frames: &mut self.frames
        }
    }
}

pub struct Iter<'a> {
    frames: &'a mut Frames
}

impl<'a> Iterator for Iter<'a> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Vec<u8>> {
        self.frames.completed_frames.pop_front()
    }
}

#[derive(Debug)]
struct Frames {
    max_frame_size: u32,
    bytes_read: usize,
    header: [u8; 4],
    reading_header: bool,
    current: Vec<u8>,
    completed_frames: VecDeque<Vec<u8>>
}

impl Frames {
    pub fn new(max_frame_size: u32) -> Frames {
        Frames {
            max_frame_size: max_frame_size,
            bytes_read: 0,
            header: [0; 4],
            reading_header: true,
            current: Vec::with_capacity(0),
            completed_frames: VecDeque::new()
        }
    }

    fn read<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        let mut total_bytes_read = 0;
        loop {
            let bytes_read = try!(self.do_read(reader));
            total_bytes_read += bytes_read;

            if total_bytes_read == 0 {
                return Err(Error::new(ErrorKind::UnexpectedEof, "Read 0 bytes"));
            }

            // We either are done reading or we read a bunch of data, and didn't exaust our buffer.
            if bytes_read == 0 || self.bytes_read != 0 {
                return Ok(total_bytes_read)
            }
        }
    }

    fn do_read<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        if self.reading_header {
            self.read_header(reader)
        } else {
            self.read_value(reader)
        }
    }

    // TODO: Return an error if size is greater than max_frame_size
    fn read_header<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        let bytes_read = try!(reader.read(&mut self.header[self.bytes_read..]));
        self.bytes_read += bytes_read;
        if self.bytes_read == 4 {
           let len = unsafe { u32::from_be(mem::transmute(self.header)) };
           self.bytes_read = 0;
           self.reading_header = false;
           self.current = Vec::with_capacity(len as usize);
           unsafe { self.current.set_len(len as usize); }
        }
        Ok(bytes_read)
    }

    fn read_value<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        let bytes_read = try!(reader.read(&mut self.current[self.bytes_read..]));
        self.bytes_read += bytes_read;
        if self.bytes_read == self.current.len() {
           self.completed_frames.push_back(mem::replace(&mut self.current, Vec::new()));
           self.bytes_read = 0;
           self.reading_header = true;
        }
        Ok(bytes_read)
    }
}

#[cfg(test)]
mod tests {
    use std::mem;
    use std::io::Cursor;
    use std::io::Write;
    use super::FrameReader;

    #[test]
    fn partial_and_complete_reads() {
        let buf1 = String::from("Hello World").into_bytes();
        let buf2 = String::from("Hi.").into_bytes();
        let header1: [u8; 4] = unsafe { mem::transmute((buf1.len() as u32).to_be()) };
        let header2: [u8; 4] = unsafe { mem::transmute((buf2.len() as u32).to_be()) };

        let mut reader = FrameReader::new(1024);

        // Write a partial header
        let mut header = Cursor::new(&header1[0..2]);
        let bytes_read = reader.read(&mut header).unwrap();
        assert_eq!(2, bytes_read);
        assert_eq!(None, reader.iter_mut().next());

        // Complete writing just the header
        let mut header = Cursor::new(&header1[2..]);
        let bytes_read = reader.read(&mut header).unwrap();
        assert_eq!(2, bytes_read);
        assert_eq!(None, reader.iter_mut().next());

        // Write a partial value
        let mut data = Cursor::new(&buf1[0..5]);
        let bytes_read = reader.read(&mut data).unwrap();
        assert_eq!(5, bytes_read);
        assert_eq!(None, reader.iter_mut().next());

        // Complete writing the first value
        let mut data = Cursor::new(&buf1[5..]);
        let bytes_read = reader.read(&mut data).unwrap();
        assert_eq!(6, bytes_read);
        let val = reader.iter_mut().next().unwrap();
        assert_eq!(buf1, val);

        // Write an entire header and value
        let mut data = Cursor::new(Vec::with_capacity(7));
        assert_eq!(4, data.write(&header2).unwrap());
        assert_eq!(3, data.write(&buf2).unwrap());
        data.set_position(0);
        let bytes_read = reader.read(&mut data).unwrap();
        assert_eq!(7, bytes_read);
        assert_eq!(buf2, reader.iter_mut().next().unwrap());
    }
}
