# Bugfix Requirements Document

## Introduction

echOS projesinde `cargo build` çalıştırıldığında derleme başarısız oluyor. Altı farklı dosyada tip tanımları, fonksiyon görünürlüğü ve eksik fonksiyon implementasyonları nedeniyle hatalar alınıyor. Bu hatalar projenin derlenmesini tamamen engelliyor ve düzeltilmesi gerekiyor.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN `cargo build` çalıştırıldığında THEN `hotplug.rs` dosyasında `AtomicU64` tipi tanımlı değil hatası alınıyor

1.2 WHEN `cargo build` çalıştırıldığında THEN `rcu.rs` dosyasında `start_grace_period` fonksiyonu private olduğu için `atomic_ops.rs` dosyasından erişilemiyor

1.3 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `start_cpu` fonksiyonu tanımlı değil hatası alınıyor

1.4 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `stop_cpu` fonksiyonu tanımlı değil hatası alınıyor

1.5 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `get_cpu_count` fonksiyonu tanımlı değil hatası alınıyor

1.6 WHEN `cargo build` çalıştırıldığında THEN `atomic_ops.rs` dosyasında `Box` tipi bazı yerlerde tanımlı değil hatası alınıyor

1.7 WHEN `cargo build` çalıştırıldığında THEN `task/scheduler.rs` dosyasında type annotation hataları alınıyor

### Expected Behavior (Correct)

2.1 WHEN `cargo build` çalıştırıldığında THEN `hotplug.rs` dosyasında `core::sync::atomic::AtomicU64` import edilmiş olmalı ve derleme hatası alınmamalı

2.2 WHEN `cargo build` çalıştırıldığında THEN `rcu.rs` dosyasında `start_grace_period` fonksiyonu `pub` olarak işaretlenmiş olmalı ve diğer modüllerden erişilebilir olmalı

2.3 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `start_cpu` fonksiyonu tanımlanmış ve `hotplug.rs` tarafından çağrılabilir olmalı

2.4 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `stop_cpu` fonksiyonu tanımlanmış ve `hotplug.rs` tarafından çağrılabilir olmalı

2.5 WHEN `cargo build` çalıştırıldığında THEN `smp.rs` dosyasında `get_cpu_count` fonksiyonu tanımlanmış ve `rcu.rs` tarafından çağrılabilir olmalı

2.6 WHEN `cargo build` çalıştırıldığında THEN `atomic_ops.rs` dosyasında `alloc::boxed::Box` import edilmiş olmalı ve derleme hatası alınmamalı

2.7 WHEN `cargo build` çalıştırıldığında THEN `task/scheduler.rs` dosyasındaki type annotation hataları düzeltilmiş olmalı

### Unchanged Behavior (Regression Prevention)

3.1 WHEN derleme başarılı olduktan sonra THEN mevcut tüm fonksiyonların davranışları değişmeden korunmalı

3.2 WHEN `hotplug.rs` dosyasında `AtomicU64` import edildikten sonra THEN dosyadaki diğer atomic operasyonlar etkilenmemeli

3.3 WHEN `rcu.rs` dosyasında `start_grace_period` public yapıldıktan sonra THEN RCU mekanizmasının iç çalışma mantığı değişmemeli

3.4 WHEN `smp.rs` dosyasına yeni fonksiyonlar eklendikten sonra THEN mevcut SMP başlatma ve yönetim fonksiyonları etkilenmemeli

3.5 WHEN `atomic_ops.rs` dosyasında `Box` import edildikten sonra THEN mevcut atomic operasyonlar ve lock-free veri yapıları etkilenmemeli

3.6 WHEN `task/scheduler.rs` dosyasındaki type annotation hataları düzeltildikten sonra THEN scheduler'ın task zamanlama mantığı değişmemeli
