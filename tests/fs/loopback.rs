//! # Wave 5.9.4 — Loopback Corpus
//!
//! Host-side simulation of loopback device operations: attach/detach,
//! flush, large image handling, backend error propagation, mount cycle, read-only.

#![cfg(not(target_os = "none"))]

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoopError {
    NotAttached,
    AlreadyAttached,
    IoError,
    ReadOnly,
    BackendError(String),
}

struct LoopbackDevice {
    attached: bool,
    image_size: usize,
    paged_mode: bool,
    read_only: bool,
    dirty_blocks: HashMap<u64, Vec<u8>>,
    block_size: usize,
    backend_data: HashMap<u64, Vec<u8>>,
    mounted: bool,
}

impl LoopbackDevice {
    fn new(block_size: usize) -> Self {
        Self {
            attached: false,
            image_size: 0,
            paged_mode: false,
            read_only: false,
            dirty_blocks: HashMap::new(),
            block_size,
            backend_data: HashMap::new(),
            mounted: false,
        }
    }

    fn attach(&mut self, image_size: usize) -> Result<(), LoopError> {
        if self.attached {
            return Err(LoopError::AlreadyAttached);
        }
        self.attached = true;
        self.image_size = image_size;
        self.paged_mode = image_size > 64 * 1024 * 1024;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), LoopError> {
        if !self.attached {
            return Err(LoopError::NotAttached);
        }
        self.flush()?;
        self.attached = false;
        self.image_size = 0;
        self.paged_mode = false;
        self.dirty_blocks.clear();
        Ok(())
    }

    fn flush(&mut self) -> Result<(), LoopError> {
        if !self.attached {
            return Err(LoopError::NotAttached);
        }
        for (block, data) in self.dirty_blocks.drain() {
            self.backend_data.insert(block, data);
        }
        Ok(())
    }

    fn read_block(&mut self, block_num: u64) -> Result<Vec<u8>, LoopError> {
        if !self.attached {
            return Err(LoopError::NotAttached);
        }
        let max_block = (self.image_size as u64).div_ceil(self.block_size as u64);
        if block_num >= max_block {
            return Err(LoopError::BackendError(format!(
                "block {} exceeds image size",
                block_num
            )));
        }

        if let Some(data) = self.dirty_blocks.get(&block_num) {
            return Ok(data.clone());
        }
        if let Some(data) = self.backend_data.get(&block_num) {
            return Ok(data.clone());
        }

        Ok(vec![0u8; self.block_size])
    }

    fn write_block(&mut self, block_num: u64, data: &[u8]) -> Result<(), LoopError> {
        if !self.attached {
            return Err(LoopError::NotAttached);
        }
        if self.read_only {
            return Err(LoopError::ReadOnly);
        }
        let max_block = (self.image_size as u64).div_ceil(self.block_size as u64);
        if block_num >= max_block {
            return Err(LoopError::BackendError(format!(
                "block {} exceeds image size",
                block_num
            )));
        }

        self.dirty_blocks.insert(block_num, data.to_vec());
        Ok(())
    }

    fn mount(&mut self) -> Result<(), LoopError> {
        if !self.attached {
            return Err(LoopError::NotAttached);
        }
        self.mounted = true;
        Ok(())
    }

    fn unmount(&mut self) -> Result<(), LoopError> {
        if !self.mounted {
            return Err(LoopError::NotAttached);
        }
        self.flush()?;
        self.mounted = false;
        Ok(())
    }
}

#[test]
fn attach_detach() {
    let mut dev = LoopbackDevice::new(4096);

    assert!(dev.attach(1024 * 1024).is_ok());
    assert!(dev.attached);
    assert!(!dev.paged_mode);

    assert!(dev.detach().is_ok());
    assert!(!dev.attached);
    assert!(dev.dirty_blocks.is_empty());

    assert_eq!(dev.detach(), Err(LoopError::NotAttached));
}

#[test]
fn flush() {
    let mut dev = LoopbackDevice::new(4096);
    dev.attach(1024 * 1024).unwrap();

    let test_data = vec![0xABu8; 4096];
    dev.write_block(0, &test_data).unwrap();

    assert!(dev.dirty_blocks.contains_key(&0));
    assert!(!dev.backend_data.contains_key(&0));

    dev.flush().unwrap();

    assert!(dev.dirty_blocks.is_empty());
    assert!(dev.backend_data.contains_key(&0));
    assert_eq!(dev.backend_data[&0], test_data);
}

#[test]
fn large_image() {
    let mut dev = LoopbackDevice::new(4096);

    let large_size = 128 * 1024 * 1024;
    dev.attach(large_size).unwrap();

    assert!(dev.paged_mode);
    assert_eq!(dev.image_size, large_size);

    let max_block = (large_size as u64).div_ceil(4096u64);
    let last_block = max_block - 1;

    let data = vec![0xCDu8; 4096];
    dev.write_block(last_block, &data).unwrap();
    let read = dev.read_block(last_block).unwrap();
    assert_eq!(read, data);

    assert!(dev.read_block(max_block).is_err());
}

#[test]
fn backend_error() {
    let mut dev = LoopbackDevice::new(4096);
    dev.attach(4096 * 10).unwrap();

    let result = dev.read_block(100);
    assert!(result.is_err());
    match result {
        Err(LoopError::BackendError(msg)) => assert!(msg.contains("exceeds")),
        _ => panic!("expected BackendError"),
    }

    let result2 = dev.write_block(100, &[0u8; 4096]);
    assert!(result2.is_err());
}

#[test]
fn mount_unmount() {
    let mut dev = LoopbackDevice::new(4096);
    dev.attach(1024 * 1024).unwrap();

    assert!(dev.mount().is_ok());
    assert!(dev.mounted);

    let data = vec![0xEFu8; 4096];
    dev.write_block(0, &data).unwrap();

    assert!(dev.unmount().is_ok());
    assert!(!dev.mounted);
    assert!(dev.backend_data.contains_key(&0));

    assert_eq!(dev.unmount(), Err(LoopError::NotAttached));
}

#[test]
fn read_only() {
    let mut dev = LoopbackDevice::new(4096);
    dev.attach(1024 * 1024).unwrap();
    dev.read_only = true;

    let result = dev.write_block(0, &[0u8; 4096]);
    assert_eq!(result, Err(LoopError::ReadOnly));

    let read_result = dev.read_block(0);
    assert!(read_result.is_ok());
}
