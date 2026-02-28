#![cfg(not(target_os = "none"))]
//! Görev Zamanlama Benchmark Takımı - echOS ile Linux CFS ve Windows Zamanlayıcı karşılaştırması
//!
//! Bu modül, echOS'un görev zamanlayıcısının performansını ölçer.
//! Zamanlayıcı, hangi görevin ne zaman CPU üzerinde çalışacağına karar verir.
//!
//! Ölçülen metrikler:
//!   - Verim (throughput): Birim zamanda tamamlanan görev sayısı
//!   - Gecikme (latency): Görev kuyruğa alınmasından çalıştırılmasına geçen süre
//!   - Adillik (fairness): Farklı önceliklerdeki görevlere eşit CPU payı
//!
//! Öncelik seviyeleri:
//!   RealTime > High > Normal > Low
//!    (0)       (1)    (2)      (3)

#![feature(test)]
extern crate test;

use ech_os::task::scheduler::Scheduler;
use ech_os::task::{Task, TaskPriority};
use test::Bencher;

/// `bench_scheduler_throughput`: Zamanlayıcı verimini ölçer.
///
/// Bu benchmark, 1000 farklı öncelikli görev oluşturarak
/// zamanlayıcının 1000 zaman dilimi (time slice) içinde ne kadar
/// iş tamamladığını ölçer.
///
/// Görev dağılımı (önceliğe göre döngüsel):
///   i % 4 == 0  → RealTime  (250 görev)
///   i % 4 == 1  → High      (250 görev)
///   i % 4 == 2  → Normal    (250 görev)
///   i % 4 == 3  → Low       (250 görev)
///
/// Her görev CPU yoğun iş simüle eder (wrapping_add ile taşma güvenli toplam).
#[bench]
fn bench_scheduler_throughput(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();

        // 1000 farklı öncelikli görev oluştur
        for i in 0..1000 {
            let priority = match i % 4 {
                0 => TaskPriority::RealTime,
                1 => TaskPriority::High,
                2 => TaskPriority::Normal,
                _ => TaskPriority::Low,
            };

            let task = Task::new(
                move || {
                    // CPU yoğun iş simülasyonu
                    let mut result = 0;
                    for j in 0..1000 {
                        result = result.wrapping_add(j);
                    }
                    result
                },
                priority,
            );

            scheduler.add_task(task);
        }

        // Zamanlayıcıyı 1000 zaman dilimi çalıştır
        let mut total_work = 0;
        for _ in 0..1000 {
            if let Some(mut task) = scheduler.next_task() {
                total_work += task.run();
            }
        }

        test::black_box(total_work);
    });
}

/// `bench_scheduler_latency`: Gerçek zamanlı görev gecikme süresini ölçer.
///
/// Gerçek zamanlı (RealTime) görevler, zamanlayıcıya eklendiği andan
/// çalıştırıldığı ana kadar geçen süre ile ölçülür.
///
/// Gecikme ölçüm yöntemi (RDTSC tabanlı):
///   start = rdtsc()   ← Görev başlamadan hemen önce CPU sayaç değeri
///   ... iş yap ...
///   end = rdtsc()     ← Görev bittikten hemen sonra CPU sayaç değeri
///   gecikme = end - start  (CPU döngüsü cinsinden)
///
/// RDTSC (Read Time-Stamp Counter): x86_64 işlemcisinin dahili yüksek
/// çözünürlüklü zamanlayıcısını okur. std::time'dan daha hassastır
/// çünkü doğrudan donanım sayacını sorgular.
#[bench]
fn bench_scheduler_latency(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();

        // Gerçek zamanlı görevler oluştur
        for i in 0..100 {
            let task = Task::new(
                move || {
                    // RDTSC ile döngü sayısını ölçerek gecikmeyi hesapla
                    let start = x86_64::instructions::rdtsc();
                    let mut result = 0;
                    for j in 0..100 {
                        result = result.wrapping_add(j);
                    }
                    let end = x86_64::instructions::rdtsc();
                    (end - start, result)
                },
                TaskPriority::RealTime,
            );

            scheduler.add_task(task);
        }

        // Zamanlama gecikmesini ölç
        let mut total_latency = 0;
        for _ in 0..100 {
            if let Some(mut task) = scheduler.next_task() {
                let (latency, _) = task.run();
                total_latency += latency;
            }
        }

        test::black_box(total_latency);
    });
}

/// `bench_scheduler_fairness`: Zamanlayıcının adillik (fairness) metriğini ölçer.
///
/// Adil bir zamanlayıcı, aynı öncelik seviyesindeki görevlere eşit CPU süresi
/// tahsis etmelidir. Bu benchmark, 4 öncelik seviyesinde her birinden 250 görev
/// oluşturarak her grubun kaç kez çalıştırıldığını sayar.
///
/// Adillik hesabı (standart sapma benzeri):
///   ort  = toplam_çalıştırma / 4
///   adillik = sqrt(Σ(sayı[i] - ort)²)
///   → Düşük değer = daha adil dağılım
///   → 0 = mükemmel adillik (tüm gruplar eşit çalıştırılmış)
///
/// Adillik karşılaştırması:
///   Linux CFS (Completely Fair Scheduler): kırmızı-siyah ağaç tabanlı, çok adil
///   Windows Scheduler: sabit öncelik + boost mekanizması, daha az adil
///   echOS: bu benchmark ile ölçülür
#[bench]
fn bench_scheduler_fairness(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();
        let mut task_execution_counts = [0; 4];

        // Her öncelik seviyesi için görevler oluştur
        for priority_level in 0..4 {
            for _ in 0..250 {
                let priority = match priority_level {
                    0 => TaskPriority::RealTime,
                    1 => TaskPriority::High,
                    2 => TaskPriority::Normal,
                    _ => TaskPriority::Low,
                };

                let task_idx = priority_level;
                let task = Task::new(
                    move || {
                        task_execution_counts[task_idx] += 1;
                    },
                    priority,
                );

                scheduler.add_task(task);
            }
        }

        // Adil dağılım testi için zamanlayıcıyı çalıştır
        for _ in 0..1000 {
            if let Some(mut task) = scheduler.next_task() {
                task.run();
            }
        }

        // Adilliği hesapla (düşük değer daha iyidir)
        let avg = task_execution_counts.iter().sum::<usize>() as f64 / 4.0;
        let fairness: f64 = task_execution_counts
            .iter()
            .map(|&count| (count as f64 - avg).powi(2))
            .sum::<f64>()
            .sqrt();

        test::black_box(fairness);
    });
}
