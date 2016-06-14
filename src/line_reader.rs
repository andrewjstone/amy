use std::io::{self, Read};
use std::str::{self, Utf8Error};

#[derive(Debug)]
struct ReaderBuf {
    vec: Vec<u8>,
    msg_start: usize,
    write_loc: usize
}

impl ReaderBuf {
    pub fn new(buffer_size: usize) -> ReaderBuf {
        let mut vec = Vec::with_capacity(buffer_size);
        unsafe { vec.set_len(buffer_size); }
        ReaderBuf {
            vec: vec,
            msg_start: 0,
            write_loc: 0
        }
    }

    /// Take any unread bytes and move them to the beginning of the buffer.
    /// Reset the msg_start to 0 and write_loc to the length of the unwritten bytes.
    /// This is useful when there is an incomplete messge, and we want to continue
    /// reading new bytes so that we can eventually complete a message. Note that while
    /// this is somewhat inefficient, most decoders expect contiguous slices, so we can't
    /// use a ring buffer without some copying anyway. Additionally, this lets us re-use
    /// the buffer without an allocation, and is a simpler implementation.
    ///
    /// TODO: This can almost certainly be made faster with unsafe code.
    pub fn reset(&mut self) {
        let mut write_index = 0;
        for i in self.msg_start..self.write_loc {
            self.vec[write_index] = self.vec[i];
            write_index = write_index + 1;
        }
        self.write_loc = write_index;
        self.msg_start = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.write_loc == 0
    }
}

/// An iterator over lines available from the current spot in the buffer
pub struct Iter<'a> {
    buf: &'a mut ReaderBuf
}

impl<'a> Iterator for Iter<'a> {
    type Item = Result<String, Utf8Error>;

    fn next(&mut self) -> Option<Result<String, Utf8Error>> {
        if self.buf.msg_start == self.buf.write_loc {
            self.buf.reset();
            return None;
        }

        let slice = &self.buf.vec[self.buf.msg_start..self.buf.write_loc];
        match slice.iter().position(|&c| c == '\n' as u8) {
            Some(index) => {
                self.buf.msg_start = self.buf.msg_start + index + 1;
                Some(str::from_utf8(&slice[0..index+1]).map(|s| s.to_string()))
            },
            None => None
        }
    }
}

/// Read and compose lines of text
#[derive(Debug)]
pub struct LineReader {
    buf: ReaderBuf
}

impl LineReader {
    pub fn new(buffer_size: usize) -> LineReader {
        LineReader {
            buf: ReaderBuf::new(buffer_size)
        }
    }

    pub fn read<T: Read>(&mut self, reader: &mut T) -> io::Result<usize> {
        let bytes_read = try!(reader.read(&mut self.buf.vec[self.buf.write_loc..]));
        self.buf.write_loc += bytes_read;
        Ok(bytes_read)
    }

    pub fn iter_mut(&mut self) -> Iter {
        Iter {
            buf: &mut self.buf
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}


#[cfg(test)]
mod tests {

    use std::io::Cursor;
    use super::LineReader;

    const TEXT: &'static str = "hello\nworld\nhow's\nit\ngoing?\n";

    #[test]
    fn static_buffer_single_read() {
        let mut data = Cursor::new(TEXT);
        let mut line_reader = LineReader::new(1024);
        let bytes_read = line_reader.read(&mut data).unwrap();
        assert_eq!(false, line_reader.is_empty());
        assert_eq!(TEXT.len(), bytes_read);
        assert_eq!(5, line_reader.iter_mut().count());
        assert_eq!(None, line_reader.iter_mut().next());
        assert_eq!(true, line_reader.is_empty());
    }

    #[test]
    fn static_buffer_partial_read_follow_by_complete_read() {
        let mut string = TEXT.to_string();
        string.push_str("ok");
        let mut data = Cursor::new(&string);
        let mut line_reader = LineReader::new(1024);
        let bytes_read = line_reader.read(&mut data).unwrap();
        assert_eq!(false, line_reader.is_empty());
        assert_eq!(string.len(), bytes_read);
        assert_eq!(5, line_reader.iter_mut().count());
        assert_eq!(None, line_reader.iter_mut().next());
        assert_eq!(false, line_reader.is_empty());
        assert_eq!(1, line_reader.read(&mut Cursor::new("\n")).unwrap());
        assert_eq!("ok\n".to_string(), line_reader.iter_mut().next().unwrap().unwrap());
        assert_eq!(None, line_reader.iter_mut().next());
        assert_eq!(true, line_reader.is_empty());
    }

}
