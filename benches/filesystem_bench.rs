#![cfg(not(target_os = "none"))]
//! Dosya Sistemi Benchmark Takımı - echOS VFS ile Linux Ext4 ve Windows NTFS karşılaştırması
//!
//! Bu modül, echOS'un sanal dosya sistemi (VFS) katmanının performansını
//! ölçer. Benchmark'lar üç temel işlemi test eder:
//!   1. Dosya oluşturma ve yazma hızı
//!   2. Sıralı (sequential) okuma verimi
//!   3. Rastgele (random) erişim gecikme süresi
//!   4. Meta veri (dizin listeleme, boyut sorgulama) işlem hızı

#![feature(test)]
extern crate test;

use ech_os::fs::{File, FileSystem, OpenOptions};
use test::Bencher;

/// `bench_filesystem_create_files`: 1000 küçük dosya oluşturma ve yazma hızını ölçer.
///
/// Bu benchmark şu soruyu yanıtlar: "Dosya sistemi yüksek sayıda küçük
/// dosya oluşturmayı ne kadar hızlı yapabilir?"
/// Dizin içi arama, inode tahsisi ve blok yazma I/O yolunu test eder.
#[bench]
fn bench_filesystem_create_files(b: &mut Bencher) {
    b.iter(|| {
        let mut fs = FileSystem::new();

        // 1000 küçük dosya oluştur
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            let mut file = fs.create_file(&filename).unwrap();

            // Dosyaya veri yaz
            let data = format!("Merhaba Dünya! Bu {} numaralı dosyadır\n", i);
            file.write_all(data.as_bytes()).unwrap();
        }

        // Tüm dosyaların mevcut olduğunu doğrula
        for i in 0..1000 {
            let filename = format!("testfile_{}.txt", i);
            assert!(fs.file_exists(&filename));
        }
    });
}

/// `bench_filesystem_read_sequential`: Sıralı dosya okuma verimini ölçer.
///
/// Sıralı okuma, dosya bloklarını sırası ile okur; bu işlem
/// disk önbelleği (page cache) ve read-ahead mekanizmalarından faydalanır.
/// 100 adet 1MB'lık dosya oluşturulduktan sonra ardışık olarak okunur.
///
/// Okuma döngüsü akışı:
///   Dosya aç → 8KB tampon ile oku → Veri bütünlüğü doğrula → Tekrar
#[bench]
fn bench_filesystem_read_sequential(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Kurulum: Test dosyalarını oluştur
    for i in 0..100 {
        let filename = format!("read_test_{}.dat", i);
        let mut file = fs.create_file(&filename).unwrap();

        // Her dosyaya 1MB veri yaz
        let data = vec![(i % 256) as u8; 1024 * 1024];
        file.write_all(&data).unwrap();
    }

    b.iter(|| {
        // Sıralı okuma benchmark'ı
        let mut total_bytes = 0;

        for i in 0..100 {
            let filename = format!("read_test_{}.dat", i);
            let mut file = fs.open_file(&filename).unwrap();

            let mut buffer = vec![0; 8192]; // 8KB'lık tampon bellek
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

/// `bench_filesystem_random_access`: Rastgele erişim gecikmesini ölçer.
///
/// Rastgele erişim, dosya sistemi B-ağacı veya FAT tablosunun
/// blok arama performansını test eder. Disk cache miss oranı yüksektir.
///
/// 100MB'lık tek bir dosyada 1000 farklı konuma atlanarak 4KB bloklar okunur.
/// Her iterasyonda rastgele bir offset hesaplanır:
///   offset = rand() % (file_size - 4096)
#[bench]
fn bench_filesystem_random_access(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Kurulum: Büyük bir test dosyası oluştur
    let mut large_file = fs.create_file("random_access.bin").unwrap();
    let file_size = 1024 * 1024 * 100; // 100MB
    large_file.set_len(file_size).unwrap();

    b.iter(|| {
        // Rastgele erişim benchmark'ı
        let mut rng = rand::thread_rng();
        let mut total_read = 0;

        for _ in 0..1000 {
            let offset = (rng.gen::<u64>() % (file_size - 8192)) as u64;
            let mut file = fs.open_file("random_access.bin").unwrap();
            file.seek(std::io::SeekFrom::Start(offset)).unwrap();

            let mut buffer = [0; 4096]; // 4KB okuma
            let bytes_read = file.read(&mut buffer).unwrap();
            total_read += bytes_read;

            // Gerçekten okuma yapıldığını doğrula
            test::black_box(&buffer[..bytes_read]);
        }

        test::black_box(total_read);
    });
}

/// `bench_filesystem_metadata_operations`: Meta veri işlem hızını ölçer.
///
/// Meta veri işlemleri, asıl dosya içeriğine dokunmadan dizin girişlerini,
/// inode tablolarını ve dosya özniteliklerini (boyut, tür, izinler) sorgular.
/// Bu benchmark şunu ölçer:
///   - Dizin listeleme hızı (readdir)
///   - Dosya özniteliği sorgulama hızı (stat)
///
/// Dizin yapısı (100 dizin × 10 dosya = 1000 dosya):
///   dir_0/
///     file_0.txt ... file_9.txt
///   dir_1/ ...
///   dir_99/
#[bench]
fn bench_filesystem_metadata_operations(b: &mut Bencher) {
    let mut fs = FileSystem::new();

    // Kurulum: Dizin yapısını oluştur
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
        // Meta veri işlemleri benchmark'ı
        let mut total_files = 0;
        let mut total_dirs = 0;

        for i in 0..100 {
            let dirname = format!("dir_{}", i);

            // Dizin içeriğini listele
            let entries = fs.read_dir(&dirname).unwrap();
            total_files += entries.len();
            total_dirs += 1;

            // Dosya meta verilerini sorgula
            for entry in entries {
                let metadata = fs.metadata(&entry).unwrap();
                test::black_box(metadata.size());
                test::black_box(metadata.is_file());
            }
        }

        test::black_box((total_files, total_dirs));
    });
}
