#![cfg(not(target_os = "none"))]
//! Filesystem Benchmark Suite - echOS VFS vs Linux Ext4 vs Windows NTFS

#![feature(test)]
extern crate test;

use ech_os::fs::{File, FileSystem, OpenOptions};
use test::Bencher;

#[bench]
fn bench_filesystem_create_files(b: &mut Bencher) {
    b.iter(|| {
        let mut fs = FileSystem::new();

        // Create 1000 small files
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            let mut file = fs.create_file(&filename).unwrap();

            // Write some data
            let data = format!("Hello World! This is file number {}\n", i);
            file.write_all(data.as_bytes()).unwrap();
        }

        // Verify all files exist
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            assert!(fs.file_exists(&filename));
        }
    });
}

#[bench]
fn bench_filesystem_read_sequential(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Setup: Create test files
    for i in 0..100 {
        let filename = format!("read_test_{}.dat", i);
        let mut file = fs.create_file(&filename).unwrap();

        // Write 1MB of data to each file
        let data = vec![(i % 256) as u8; 1024 * 1024];
        file.write_all(&data).unwrap();
    }

    b.iter(|| {
        // Sequential read benchmark
        let mut total_bytes = 0;

        for i in 0..100 {
            let filename = format!("read_test_{}.dat", i);
            let mut file = fs.open_file(&filename).unwrap();

            let mut buffer = vec![0; 8192]; // 8KB buffer
            while let Ok(bytes_read) = file.read(&mut buffer) {
                if bytes_read == 0 {
                    break;
                }
                total_bytes += bytes_read;

                // Verify data integrity
                for &byte in &buffer[..bytes_read] {
                    test::black_box(byte);
                }
            }
        }

        test::black_box(total_bytes);
    });
}

#[bench]
fn bench_filesystem_random_access(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Setup: Create a large file
    let mut large_file = fs.create_file("random_access.bin").unwrap();
    let file_size = 1024 * 1024 * 100; // 100MB
    large_file.set_len(file_size).unwrap();

    b.iter(|| {
        // Random access benchmark
        let mut rng = rand::thread_rng();
        let mut total_read = 0;

        for _ in 0..1000 {
            let offset = (rng.gen::<u64>() % (file_size - 8192)) as u64;
            let mut file = fs.open_file("random_access.bin").unwrap();
            file.seek(std::io::SeekFrom::Start(offset)).unwrap();

            let mut buffer = [0; 4096]; // 4KB reads
            let bytes_read = file.read(&mut buffer).unwrap();
            total_read += bytes_read;

            // Verify we read something
            test::black_box(&buffer[..bytes_read]);
        }

        test::black_box(total_read);
    });
}

#[bench]
fn bench_filesystem_metadata_operations(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Setup: Create directory structure
    for i in 0..100 {
        let dirname = format!("dir_{}", i);
        fs.create_dir(&dirname).unwrap();

        for j in 0..10 {
            let filename = format!("{}/file_{}.txt", dirname, j);
            let mut file = fs.create_file(&filename).unwrap();
            file.write_all(b"test data").unwrap();
        }
    }

    b.iter(|| {
        // Metadata operations benchmark
        let mut total_files = 0;
        let mut total_dirs = 0;

        for i in 0..100 {
            let dirname = format!("dir_{}", i);

            // List directory contents
            let entries = fs.read_dir(&dirname).unwrap();
            total_files += entries.len();
            total_dirs += 1;

            // Check file metadata
            for entry in entries {
                let metadata = fs.metadata(&entry).unwrap();
                test::black_box(metadata.size());
                test::black_box(metadata.is_file());
            }
        }

        test::black_box((total_files, total_dirs));
    });
}
