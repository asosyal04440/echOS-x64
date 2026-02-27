#![cfg(not(target_os = "none"))]
//! Dosya Sistemi Kıyaslama Paketi - echOS VFS ile Linux Ext4 ve Windows NTFS karşılaştırması
//!
//! Bu modül; dosya oluşturma, sıralı/rastgele okuma ve meta veri işlemleri üzerinden
//! dosya sistemi performansını ölçen kıyaslama fonksiyonlarını içerir.

#![feature(test)]
extern crate test;

use ech_os::fs::{File, FileSystem, OpenOptions};
use test::Bencher;

#[bench]
fn bench_filesystem_create_files(b: &mut Bencher) {
    b.iter(|| {
        let mut fs = FileSystem::new();

        // 1000 adet küçük dosya oluştur
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            let mut file = fs.create_file(&filename).unwrap();

            // Bir miktar veri yaz
            let data = format!("Hello World! This is file number {}\n", i);
            file.write_all(data.as_bytes()).unwrap();
        }

        // Tüm dosyaların var olduğunu doğrula
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            assert!(fs.file_exists(&filename));
        }
    });
}

#[bench]
fn bench_filesystem_read_sequential(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Hazırlık: Test dosyalarını oluştur
    for i in 0..100 {
        let filename = format!("read_test_{}.dat", i);
        let mut file = fs.create_file(&filename).unwrap();

        // Her dosyaya 1 MB veri yaz
        let data = vec![(i % 256) as u8; 1024 * 1024];
        file.write_all(&data).unwrap();
    }

    b.iter(|| {
        // Sıralı okuma kıyaslaması
        let mut total_bytes = 0;

        for i in 0..100 {
            let filename = format!("read_test_{}.dat", i);
            let mut file = fs.open_file(&filename).unwrap();

            let mut buffer = vec![0; 8192]; // 8 KB'lık arabellek
            while let Ok(bytes_read) = file.read(&mut buffer) {
                if bytes_read == 0 {
                    break;
                }
                total_bytes += bytes_read;

                // Veri bütünlüğünü doğrula
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

    // Hazırlık: Büyük bir dosya oluştur
    let mut large_file = fs.create_file("random_access.bin").unwrap();
    let file_size = 1024 * 1024 * 100; // 100 MB
    large_file.set_len(file_size).unwrap();

    b.iter(|| {
        // Rastgele erişim kıyaslaması
        let mut rng = rand::thread_rng();
        let mut total_read = 0;

        for _ in 0..1000 {
            let offset = (rng.gen::<u64>() % (file_size - 8192)) as u64;
            let mut file = fs.open_file("random_access.bin").unwrap();
            file.seek(std::io::SeekFrom::Start(offset)).unwrap();

            let mut buffer = [0; 4096]; // 4 KB'lık okuma
            let bytes_read = file.read(&mut buffer).unwrap();
            total_read += bytes_read;

            // Bir şeyler okunduğunu doğrula
            test::black_box(&buffer[..bytes_read]);
        }

        test::black_box(total_read);
    });
}

#[bench]
fn bench_filesystem_metadata_operations(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Hazırlık: Dizin yapısını oluştur
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
        // Meta veri işlemleri kıyaslaması
        let mut total_files = 0;
        let mut total_dirs = 0;

        for i in 0..100 {
            let dirname = format!("dir_{}", i);

            // Dizin içeriğini listele
            let entries = fs.read_dir(&dirname).unwrap();
            total_files += entries.len();
            total_dirs += 1;

            // Dosya meta verilerini kontrol et
            for entry in entries {
                let metadata = fs.metadata(&entry).unwrap();
                test::black_box(metadata.size());
                test::black_box(metadata.is_file());
            }
        }

        test::black_box((total_files, total_dirs));
    });
}
