#![cfg(not(target_os = "none"))]
//! Görev Zamanlama Kıyaslama Paketi - echOS vs Linux CFS vs Windows Schedulerı
//!
//! Bu modül; görev oluşturma verimi, gerçek zamanlı gecikme ölçümü ve
//! öncelik bazlı adil dağılım (fairness) testlerini içerir.

#![feature(test)]
extern crate test;

use ech_os::task::scheduler::Scheduler;
use ech_os::task::{Task, TaskPriority};
use test::Bencher;

#[bench]
fn bench_scheduler_throughput(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();

        // Farklı önceliklerle 1000 görev oluştur
        for i in 0..1000 {
            let priority = match i % 4 {
                0 => TaskPriority::RealTime,
                1 => TaskPriority::High,
                2 => TaskPriority::Normal,
                _ => TaskPriority::Low,
            };

            let task = Task::new(
                move || {
                    // İşlemci yoğun iş
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

        // Zamanlayıcıyı 1000 zaman dilimi boyunca çalıştır
        let mut total_work = 0;
        for _ in 0..1000 {
            if let Some(mut task) = scheduler.next_task() {
                total_work += task.run();
            }
        }

        test::black_box(total_work);
    });
}

#[bench]
fn bench_scheduler_latency(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();

        // Gerçek zamanlı görevler oluştur
        for i in 0..100 {
            let task = Task::new(
                move || {
                    // Döngü sayarak gecikmeyi ölç
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

#[bench]
fn bench_scheduler_fairness(b: &mut Bencher) {
    b.iter(|| {
        let mut scheduler = Scheduler::new();
        let mut task_execution_counts = [0; 4];

        // Her öncelik seviyesi için görev oluştur
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
