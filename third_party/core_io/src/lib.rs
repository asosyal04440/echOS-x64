#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    NotFound,
    AlreadyExists,
    InvalidInput,
    UnexpectedEof,
    WriteZero,
    Other,
}

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
}

impl Error {
    pub fn new<M: Into<String>>(kind: ErrorKind, message: M) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

pub struct Cursor<T> {
    inner: T,
    position: u64,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, position: 0 }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn set_position(&mut self, pos: u64) {
        self.position = pos;
    }
}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => {
                    return Err(Error::new(ErrorKind::UnexpectedEof, "failed to fill whole buffer"))
                }
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let mut total = 0;
        let mut chunk = [0u8; 1024];
        loop {
            let read = self.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..read]);
            total += read;
        }
        Ok(total)
    }
}

impl<T: Read + ?Sized> Read for &mut T {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}

impl<T: AsRef<[u8]>> Read for Cursor<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let data = self.inner.as_ref();
        let pos = self.position as usize;
        if pos >= data.len() {
            return Ok(0);
        }
        let available = data.len() - pos;
        let to_copy = core::cmp::min(buf.len(), available);
        buf[..to_copy].copy_from_slice(&data[pos..pos + to_copy]);
        self.position += to_copy as u64;
        Ok(to_copy)
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => {
                    return Err(Error::new(ErrorKind::WriteZero, "failed to write whole buffer"))
                }
                Ok(n) => buf = &buf[n..],
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}

impl<T: Write + ?Sized> Write for &mut T {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

impl Write for Cursor<Vec<u8>> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let pos = self.position as usize;
        if pos > self.inner.len() {
            self.inner.resize(pos, 0);
        }
        let end = pos.saturating_add(buf.len());
        if end > self.inner.len() {
            self.inner.resize(end, 0);
        }
        self.inner[pos..end].copy_from_slice(buf);
        self.position = end as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl<'a> Write for Cursor<&'a mut [u8]> {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let pos = self.position as usize;
        if pos > self.inner.len() {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid cursor position"));
        }
        let available = self.inner.len() - pos;
        let to_copy = core::cmp::min(buf.len(), available);
        self.inner[pos..pos + to_copy].copy_from_slice(&buf[..to_copy]);
        self.position += to_copy as u64;
        Ok(to_copy)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub trait Seek {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;
}

impl<T: Seek + ?Sized> Seek for &mut T {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        (**self).seek(pos)
    }
}

impl<T: AsRef<[u8]>> Seek for Cursor<T> {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64> {
        let len = self.inner.as_ref().len() as i64;
        let next = match pos {
            SeekFrom::Start(value) => value as i64,
            SeekFrom::End(value) => len.saturating_add(value),
            SeekFrom::Current(value) => self.position as i64 + value,
        };
        if next < 0 {
            return Err(Error::new(ErrorKind::InvalidInput, "invalid seek"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

pub mod prelude {
    pub use super::{Read, Seek, Write};
}
