//! # Wave 5.9.8 — fsx-style Random I/O Test
//!
//! Host-side simulation of fsx-style random I/O: random read/write/truncate/fsync
//! operations with data integrity verification after each operation.

#![cfg(not(target_os = "none"))]

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;

const DEFAULT_ITERATIONS: usize = 1000;
const MAX_FILE_SIZE: usize = 64 * 1024;
const MAX_OP_SIZE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
enum FsxError {
    IoError,
    DataCorruption,
    BoundsError,
}

struct SimFile {
    data: Vec<u8>,
    size: usize,
}

impl SimFile {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            size: 0,
        }
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize, FsxError> {
        if offset >= self.size {
            return Ok(0);
        }
        let available = self.size - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&self.data[offset..offset + to_read]);
        Ok(to_read)
    }

    fn write_at(&mut self, offset: usize, data: &[u8]) -> Result<usize, FsxError> {
        let end = offset + data.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(data);
        if end > self.size {
            self.size = end;
        }
        Ok(data.len())
    }

    fn truncate(&mut self, new_size: usize) -> Result<(), FsxError> {
        self.data.resize(new_size, 0);
        self.size = new_size;
        Ok(())
    }

    fn fsync(&mut self) -> Result<(), FsxError> {
        Ok(())
    }
}

struct GoldenBuffer {
    data: Vec<u8>,
    size: usize,
}

impl GoldenBuffer {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            size: 0,
        }
    }

    fn write_at(&mut self, offset: usize, data: &[u8]) {
        let end = offset + data.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(data);
        if end > self.size {
            self.size = end;
        }
    }

    fn truncate(&mut self, new_size: usize) {
        self.data.resize(new_size, 0);
        self.size = new_size;
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        if offset >= self.size {
            return 0;
        }
        let available = self.size - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&self.data[offset..offset + to_read]);
        to_read
    }
}

#[derive(Debug, Clone, Copy)]
enum FsxOp {
    Read,
    Write,
    Truncate,
    Fsync,
}

struct FsxTest {
    file: SimFile,
    golden: GoldenBuffer,
    rng: SmallRng,
    iterations: usize,
    max_file_size: usize,
    max_op_size: usize,
}

impl FsxTest {
    fn new(seed: u64, iterations: usize) -> Self {
        Self {
            file: SimFile::new(),
            golden: GoldenBuffer::new(),
            rng: SmallRng::seed_from_u64(seed),
            iterations,
            max_file_size: MAX_FILE_SIZE,
            max_op_size: MAX_OP_SIZE,
        }
    }

    fn random_op(&mut self) -> Result<(), FsxError> {
        let op_idx = self.rng.gen_range(0..4);
        let op = match op_idx {
            0 => FsxOp::Read,
            1 => FsxOp::Write,
            2 => FsxOp::Truncate,
            _ => FsxOp::Fsync,
        };

        match op {
            FsxOp::Read => self.do_read(),
            FsxOp::Write => self.do_write(),
            FsxOp::Truncate => self.do_truncate(),
            FsxOp::Fsync => self.do_fsync(),
        }
    }

    fn do_read(&mut self) -> Result<(), FsxError> {
        if self.file.size == 0 {
            return Ok(());
        }

        let offset = self.rng.gen_range(0..self.file.size);
        let max_len = MAX_OP_SIZE.min(self.file.size - offset);
        if max_len == 0 {
            return Ok(());
        }
        let len = self.rng.gen_range(1..=max_len);

        let mut buf = vec![0u8; len];
        let n = self.file.read_at(offset, &mut buf)?;

        let mut golden_buf = vec![0u8; len];
        let golden_n = self.golden.read_at(offset, &mut golden_buf);

        assert_eq!(n, golden_n, "read length mismatch at offset {}", offset);
        assert_eq!(
            &buf[..n],
            &golden_buf[..golden_n],
            "data corruption at offset {} len {}",
            offset,
            n
        );

        Ok(())
    }

    fn do_write(&mut self) -> Result<(), FsxError> {
        let offset = if self.file.size > 0 {
            self.rng.gen_range(0..=self.file.size)
        } else {
            0
        };
        let len = self.rng.gen_range(1..=self.max_op_size);

        let mut data = vec![0u8; len];
        self.rng.fill(&mut data[..]);

        self.file.write_at(offset, &data)?;
        self.golden.write_at(offset, &data);

        Ok(())
    }

    fn do_truncate(&mut self) -> Result<(), FsxError> {
        let new_size = self.rng.gen_range(0..=self.max_file_size);
        self.file.truncate(new_size)?;
        self.golden.truncate(new_size);
        Ok(())
    }

    fn do_fsync(&mut self) -> Result<(), FsxError> {
        self.file.fsync()?;

        assert_eq!(
            self.file.size, self.golden.size,
            "size mismatch after fsync"
        );
        assert_eq!(
            self.file.data.len(),
            self.golden.data.len(),
            "buffer length mismatch after fsync"
        );
        assert_eq!(
            self.file.data, self.golden.data,
            "data mismatch after fsync"
        );

        Ok(())
    }

    fn run(&mut self) -> Result<(), FsxError> {
        for i in 0..self.iterations {
            self.random_op().map_err(|e| {
                eprintln!("fsx error at iteration {}: {:?}", i, e);
                e
            })?;
        }

        assert_eq!(
            self.file.size, self.golden.size,
            "final size mismatch"
        );
        assert_eq!(
            self.file.data, self.golden.data,
            "final data mismatch"
        );

        Ok(())
    }
}

#[test]
fn fsx_random_io_basic() {
    let mut test = FsxTest::new(42, 100);
    test.run().unwrap();
}

#[test]
fn fsx_random_io_medium() {
    let mut test = FsxTest::new(12345, 500);
    test.run().unwrap();
}

#[test]
fn fsx_random_io_long() {
    let mut test = FsxTest::new(99999, DEFAULT_ITERATIONS);
    test.run().unwrap();
}

#[test]
fn fsx_truncate_heavy() {
    let mut test = FsxTest::new(7777, 200);
    test.max_file_size = 16 * 1024;

    for _ in 0..test.iterations {
        let op = test.rng.gen_range(0..3);
        match op {
            0 => {
                let offset = if test.file.size > 0 {
                    test.rng.gen_range(0..test.file.size)
                } else {
                    0
                };
                let len = test.rng.gen_range(1..=test.max_op_size);
                let mut data = vec![0u8; len];
                test.rng.fill(&mut data[..]);
                test.file.write_at(offset, &data).unwrap();
                test.golden.write_at(offset, &data);
            }
            1 => {
                let new_size = test.rng.gen_range(0..=test.max_file_size);
                test.file.truncate(new_size).unwrap();
                test.golden.truncate(new_size);
            }
            _ => {
                if test.file.size > 0 {
                    let offset = test.rng.gen_range(0..test.file.size);
                    let max_len = MAX_OP_SIZE.min(test.file.size - offset);
                    if max_len > 0 {
                        let len = test.rng.gen_range(1..=max_len);
                        let mut buf = vec![0u8; len];
                        let mut golden_buf = vec![0u8; len];
                        let n = test.file.read_at(offset, &mut buf).unwrap();
                        let gn = test.golden.read_at(offset, &mut golden_buf);
                        assert_eq!(n, gn);
                        assert_eq!(&buf[..n], &golden_buf[..gn]);
                    }
                }
            }
        }
    }

    assert_eq!(test.file.size, test.golden.size);
    assert_eq!(test.file.data, test.golden.data);
}

#[test]
fn fsx_write_then_verify() {
    let mut test = FsxTest::new(11111, 0);

    let data1 = b"first write content";
    test.file.write_at(0, data1).unwrap();
    test.golden.write_at(0, data1);

    let data2 = b"second write at offset 10";
    test.file.write_at(10, data2).unwrap();
    test.golden.write_at(10, data2);

    let mut buf = vec![0u8; 100];
    let n = test.file.read_at(0, &mut buf).unwrap();
    let mut golden_buf = vec![0u8; 100];
    let gn = test.golden.read_at(0, &mut golden_buf);

    assert_eq!(n, gn);
    assert_eq!(&buf[..n], &golden_buf[..gn]);
}

#[test]
fn fsx_shrink_then_read() {
    let mut test = FsxTest::new(22222, 0);

    let data = vec![0xAAu8; 1000];
    test.file.write_at(0, &data).unwrap();
    test.golden.write_at(0, &data);

    test.file.truncate(500).unwrap();
    test.golden.truncate(500);

    let mut buf = vec![0u8; 600];
    let n = test.file.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 500);

    let mut golden_buf = vec![0u8; 600];
    let gn = test.golden.read_at(0, &mut golden_buf);
    assert_eq!(n, gn);
    assert_eq!(&buf[..n], &golden_buf[..gn]);
}

#[test]
fn fsx_grow_beyond_write() {
    let mut test = FsxTest::new(33333, 0);

    let data = vec![0xBBu8; 100];
    test.file.write_at(0, &data).unwrap();
    test.golden.write_at(0, &data);

    test.file.truncate(500).unwrap();
    test.golden.truncate(500);

    assert_eq!(test.file.size, 500);
    assert_eq!(test.golden.size, 500);

    let mut buf = vec![0u8; 500];
    let n = test.file.read_at(0, &mut buf).unwrap();
    assert_eq!(n, 500);

    assert_eq!(&buf[..100], &data[..]);
    assert_eq!(&buf[100..], &[0u8; 400]);
}
