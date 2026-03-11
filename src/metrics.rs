//! # Metrics Collection System
//!
//! echOS için sistem genelinde metrik toplama sistemi.
//! CPU, memory, I/O, network ve diğer sistem metriklerini toplar.
//!
//! ## Metrics Mimarisi
//!
//! ```text
//! Application Layer
//!     │
//!     ├── Metrics API (counter, gauge, histogram)
//!     │
//! Metrics Engine
//!     │   ├── Metric Collection
//!     │   ├── Aggregation
//!     │   └── Export (Prometheus, JSON)
//!     │
//! System Layer
//!     ├── System Calls
//!     ├── Hardware Counters
//!     └── Kernel Statistics
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// METRICS SABİTLERİ
// ============================================================================

/// Maksimum metrik sayısı
pub const METRICS_MAX_METRICS: usize = 1024;

/// Maksimum sample sayısı
pub const METRICS_MAX_SAMPLES: usize = 10000;

/// Metrik tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricType {
    /// Sayaç (sadece artar)
    Counter,
    /// Ölçer (artıp azalabilir)
    Gauge,
    /// Histogram (dağılım)
    Histogram,
    /// Özet (min, max, avg, quantiles)
    Summary,
}

/// Metrik değeri
#[derive(Clone, Debug)]
pub enum MetricValue {
    /// Sayısal değer
    Numeric(u64),
    /// Kayan noktalı değer
    Float(f64),
    /// Histogram verisi
    Histogram(Vec<u64>),
    /// Özet verisi
    Summary {
        count: u64,
        sum: u64,
        min: u64,
        max: u64,
    },
}

/// Metrik etiketi
#[derive(Clone, Debug)]
pub struct MetricLabel {
    pub key: String,
    pub value: String,
}

impl MetricLabel {
    /// Yeni etiket oluştur
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
        }
    }
}

/// Metrik
#[derive(Clone, Debug)]
pub struct Metric {
    /// Metrik ID'si
    pub metric_id: u32,
    /// Adı
    pub name: String,
    /// Açıklaması
    pub description: String,
    /// Tipi
    pub metric_type: MetricType,
    /// Etiketler
    pub labels: Vec<MetricLabel>,
    /// Değer
    pub value: Mutex<MetricValue>,
    /// Oluşturulma zamanı
    pub created_time: u64,
    /// Son güncelleme zamanı
    pub last_updated: AtomicU64,
}

impl Metric {
    /// Yeni metrik oluştur
    pub fn new(metric_id: u32, name: &str, description: &str, metric_type: MetricType) -> Self {
        let initial_value = match metric_type {
            MetricType::Counter => MetricValue::Numeric(0),
            MetricType::Gauge => MetricValue::Numeric(0),
            MetricType::Histogram => MetricValue::Histogram(Vec::new()),
            MetricType::Summary => MetricValue::Summary {
                count: 0,
                sum: 0,
                min: u64::MAX,
                max: 0,
            },
        };
        
        Self {
            metric_id,
            name: name.to_string(),
            description: description.to_string(),
            metric_type,
            labels: Vec::new(),
            value: Mutex::new(initial_value),
            created_time: crate::interrupts::get_ticks(),
            last_updated: AtomicU64::new(crate::interrupts::get_ticks()),
        }
    }
    
    /// Etiket ekle
    pub fn add_label(&mut self, label: MetricLabel) {
        self.labels.push(label);
    }
    
    /// Değer güncelle
    pub fn update_value(&self, new_value: MetricValue) {
        *self.value.lock() = new_value;
        self.last_updated.store(crate::interrupts::get_ticks(), Ordering::SeqCst);
    }
    
    /// Counter'ı artır
    pub fn increment(&self, delta: u64) {
        if self.metric_type != MetricType::Counter {
            return;
        }
        
        let mut value = self.value.lock();
        if let MetricValue::Numeric(ref mut current) = *value {
            *current += delta;
        }
        
        self.last_updated.store(crate::interrupts::get_ticks(), Ordering::SeqCst);
    }
    
    /// Gauge'ı ayarla
    pub fn set_gauge(&self, value: u64) {
        if self.metric_type != MetricType::Gauge {
            return;
        }
        
        self.update_value(MetricValue::Numeric(value));
    }
    
    /// Histogram'a ekle
    pub fn observe_histogram(&self, value: u64) {
        if self.metric_type != MetricType::Histogram {
            return;
        }
        
        let mut metric_value = self.value.lock();
        if let MetricValue::Histogram(ref mut samples) = *metric_value {
            samples.push(value);
            
            // Sample sayısını sınırla
            if samples.len() > METRICS_MAX_SAMPLES {
                samples.remove(0);
            }
        }
        
        self.last_updated.store(crate::interrupts::get_ticks(), Ordering::SeqCst);
    }
    
    /// Summary'i güncelle
    pub fn observe_summary(&self, value: u64) {
        if self.metric_type != MetricType::Summary {
            return;
        }
        
        let mut metric_value = self.value.lock();
        if let MetricValue::Summary(ref mut summary) = *metric_value {
            summary.count += 1;
            summary.sum += value;
            summary.min = summary.min.min(value);
            summary.max = summary.max.max(value);
        }
        
        self.last_updated.store(crate::interrupts::get_ticks(), Ordering::SeqCst);
    }
    
    /// Değeri al
    pub fn get_value(&self) -> MetricValue {
        self.value.lock().clone()
    }
    
    /// Histogram istatistiklerini al
    pub fn get_histogram_stats(&self) -> Option<HistogramStats> {
        let value = self.value.lock();
        
        if let MetricValue::Histogram(ref samples) = *value {
            if samples.is_empty() {
                return None;
            }
            
            let mut sorted_samples = samples.clone();
            sorted_samples.sort_unstable();
            
            let count = sorted_samples.len() as u64;
            let sum: u64 = sorted_samples.iter().sum();
            let min = sorted_samples[0];
            let max = sorted_samples[sorted_samples.len() - 1];
            
            // Quantiles hesapla
            let p50 = sorted_samples[(count as f64 * 0.5) as usize];
            let p95 = sorted_samples[(count as f64 * 0.95) as usize];
            let p99 = sorted_samples[(count as f64 * 0.99) as usize];
            
            Some(HistogramStats {
                count,
                sum,
                min,
                max,
                avg: sum as f64 / count as f64,
                p50,
                p95,
                p99,
            })
        } else {
            None
        }
    }
}

/// Histogram istatistikleri
#[derive(Clone, Debug)]
pub struct HistogramStats {
    pub count: u64,
    pub sum: u64,
    pub min: u64,
    pub max: u64,
    pub avg: f64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
}

// ============================================================================
// METRICS ENGINE
// ============================================================================

/// Metrics engine
pub struct MetricsEngine {
    /// Metrikler
    pub metrics: Mutex<BTreeMap<u32, Arc<Mutex<Metric>>>>,
    /// Aktif mi?
    pub active: AtomicBool,
    /// Global istatistikler
    pub global_stats: MetricsStats,
    /// Bir sonraki metrik ID
    pub next_metric_id: AtomicU32,
}

/// Metrics istatistikleri
#[derive(Clone, Debug)]
pub struct MetricsStats {
    /// Toplam metrik sayısı
    pub total_metrics: AtomicU64,
    /// Counter sayısı
    pub counter_count: AtomicU64,
    /// Gauge sayısı
    pub gauge_count: AtomicU64,
    /// Histogram sayısı
    pub histogram_count: AtomicU64,
    /// Summary sayısı
    pub summary_count: AtomicU64,
    /// Toplam sample sayısı
    pub total_samples: AtomicU64,
}

impl MetricsStats {
    /// Yeni istatistikler oluştur
    pub fn new() -> Self {
        Self {
            total_metrics: AtomicU64::new(0),
            counter_count: AtomicU64::new(0),
            gauge_count: AtomicU64::new(0),
            histogram_count: AtomicU64::new(0),
            summary_count: AtomicU64::new(0),
            total_samples: AtomicU64::new(0),
        }
    }
}

impl Default for MetricsStats {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsEngine {
    /// Yeni metrics engine oluştur
    pub fn new() -> Self {
        Self {
            metrics: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
            global_stats: MetricsStats::new(),
            next_metric_id: AtomicU32::new(1),
        }
    }
    
    /// Metrics engine'i başlat
    pub fn init(&self) -> Result<(), &'static str> {
        if self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[METRICS] Initializing metrics engine");
        
        // Varsayılan metrikleri oluştur
        self.setup_default_metrics()?;
        
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Metrics engine initialized");
        
        Ok(())
    }
    
    /// Varsayılan metrikleri kur
    fn setup_default_metrics(&self) -> Result<(), &'static str> {
        crate::serial_println!("[METRICS] Setting up default metrics");
        
        // CPU metrikleri
        self.create_counter("cpu_cycles_total", "Total CPU cycles")?;
        self.create_counter("cpu_instructions_total", "Total CPU instructions")?;
        self.create_gauge("cpu_usage_percent", "CPU usage percentage")?;
        
        // Memory metrikleri
        self.create_gauge("memory_used_bytes", "Used memory in bytes")?;
        self.create_gauge("memory_free_bytes", "Free memory in bytes")?;
        self.create_counter("memory_allocations_total", "Total memory allocations")?;
        
        // I/O metrikleri
        self.create_counter("io_read_bytes_total", "Total bytes read")?;
        self.create_counter("io_write_bytes_total", "Total bytes written")?;
        self.create_histogram("io_latency_seconds", "I/O latency in seconds")?;
        
        // Network metrikleri
        self.create_counter("network_rx_bytes_total", "Total bytes received")?;
        self.create_counter("network_tx_bytes_total", "Total bytes transmitted")?;
        self.create_counter("network_packets_total", "Total network packets")?;
        
        // Process metrikleri
        self.create_gauge("process_count", "Number of processes")?;
        self.create_counter("context_switches_total", "Total context switches")?;
        
        Ok(())
    }
    
    /// Counter oluştur
    pub fn create_counter(&self, name: &str, description: &str) -> Result<u32, &'static str> {
        let metric_id = self.next_metric_id.fetch_add(1, Ordering::SeqCst);
        
        let metric = Arc::new(Mutex::new(Metric::new(metric_id, name, description, MetricType::Counter)));
        
        {
            let mut metrics = self.metrics.lock();
            metrics.insert(metric_id, metric);
        }
        
        self.global_stats.total_metrics.fetch_add(1, Ordering::SeqCst);
        self.global_stats.counter_count.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Created counter: {} (id={})", name, metric_id);
        
        Ok(metric_id)
    }
    
    /// Gauge oluştur
    pub fn create_gauge(&self, name: &str, description: &str) -> Result<u32, &'static str> {
        let metric_id = self.next_metric_id.fetch_add(1, Ordering::SeqCst);
        
        let metric = Arc::new(Mutex::new(Metric::new(metric_id, name, description, MetricType::Gauge)));
        
        {
            let mut metrics = self.metrics.lock();
            metrics.insert(metric_id, metric);
        }
        
        self.global_stats.total_metrics.fetch_add(1, Ordering::SeqCst);
        self.global_stats.gauge_count.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Created gauge: {} (id={})", name, metric_id);
        
        Ok(metric_id)
    }
    
    /// Histogram oluştur
    pub fn create_histogram(&self, name: &str, description: &str) -> Result<u32, &'static str> {
        let metric_id = self.next_metric_id.fetch_add(1, Ordering::SeqCst);
        
        let metric = Arc::new(Mutex::new(Metric::new(metric_id, name, description, MetricType::Histogram)));
        
        {
            let mut metrics = self.metrics.lock();
            metrics.insert(metric_id, metric);
        }
        
        self.global_stats.total_metrics.fetch_add(1, Ordering::SeqCst);
        self.global_stats.histogram_count.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Created histogram: {} (id={})", name, metric_id);
        
        Ok(metric_id)
    }
    
    /// Summary oluştur
    pub fn create_summary(&self, name: &str, description: &str) -> Result<u32, &'static str> {
        let metric_id = self.next_metric_id.fetch_add(1, Ordering::SeqCst);
        
        let metric = Arc::new(Mutex::new(Metric::new(metric_id, name, description, MetricType::Summary)));
        
        {
            let mut metrics = self.metrics.lock();
            metrics.insert(metric_id, metric);
        }
        
        self.global_stats.total_metrics.fetch_add(1, Ordering::SeqCst);
        self.global_stats.summary_count.fetch_add(1, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Created summary: {} (id={})", name, metric_id);
        
        Ok(metric_id)
    }
    
    /// Metrik al
    pub fn get_metric(&self, metric_id: u32) -> Option<Arc<Mutex<Metric>>> {
        self.metrics.lock().get(&metric_id).cloned()
    }
    
    /// Metrik ada göre ara
    pub fn find_metric_by_name(&self, name: &str) -> Option<Arc<Mutex<Metric>>> {
        let metrics = self.metrics.lock();
        
        for metric in metrics.values() {
            let m = metric.lock();
            if m.name == name {
                return Some(metric.clone());
            }
        }
        
        None
    }
    
    /// Tüm metrikleri al
    pub fn get_all_metrics(&self) -> Vec<Arc<Mutex<Metric>>> {
        self.metrics.lock().values().cloned().collect()
    }
    
    /// Metrikleri Prometheus formatında export et
    pub fn export_prometheus(&self) -> String {
        let metrics = self.metrics.lock();
        let mut output = String::new();
        
        for metric in metrics.values() {
            let m = metric.lock();
            
            // HELP ve TYPE satırları
            output.push_str(&format!("# HELP {} {}\n", m.name, m.description));
            
            let type_str = match m.metric_type {
                MetricType::Counter => "counter",
                MetricType::Gauge => "gauge",
                MetricType::Histogram => "histogram",
                MetricType::Summary => "summary",
            };
            
            output.push_str(&format!("# TYPE {} {}\n", m.name, type_str));
            
            // Değer satırları
            match m.get_value() {
                MetricValue::Numeric(value) => {
                    output.push_str(&format!("{} {}\n", m.name, value));
                }
                MetricValue::Float(value) => {
                    output.push_str(&format!("{} {}\n", m.name, value));
                }
                MetricValue::Histogram(_) => {
                    if let Some(stats) = m.get_histogram_stats() {
                        output.push_str(&format!("{}_count {}\n", m.name, stats.count));
                        output.push_str(&format!("{}_sum {}\n", m.name, stats.sum));
                        output.push_str(&format!("{}_bucket{{le=\"+Inf\"}} {}\n", m.name, stats.count));
                    }
                }
                MetricValue::Summary(summary) => {
                    output.push_str(&format!("{}_count {}\n", m.name, summary.count));
                    output.push_str(&format!("{}_sum {}\n", m.name, summary.sum));
                }
            }
        }
        
        output
    }
    
    /// Metrikleri JSON formatında export et
    pub fn export_json(&self) -> String {
        let metrics = self.metrics.lock();
        let mut output = String::from("{\n  \"metrics\": [\n");
        
        let mut first = true;
        for metric in metrics.values() {
            let m = metric.lock();
            
            if !first {
                output.push_str(",\n");
            }
            first = false;
            
            output.push_str(&format!("    {{\n      \"name\": \"{}\",\n", m.name));
            output.push_str(&format!("      \"description\": \"{}\",\n", m.description));
            output.push_str(&format!("      \"type\": \"{:?}\",\n", m.metric_type));
            
            match m.get_value() {
                MetricValue::Numeric(value) => {
                    output.push_str(&format!("      \"value\": {}", value));
                }
                MetricValue::Float(value) => {
                    output.push_str(&format!("      \"value\": {}", value));
                }
                MetricValue::Histogram(_) => {
                    if let Some(stats) = m.get_histogram_stats() {
                        output.push_str(&format!("      \"stats\": {{\"count\": {}, \"sum\": {}, \"avg\": {}}}", 
                            stats.count, stats.sum, stats.avg));
                    }
                }
                MetricValue::Summary(summary) => {
                    output.push_str(&format!("      \"summary\": {{\"count\": {}, \"sum\": {}}}", 
                        summary.count, summary.sum));
                }
            }
            
            output.push_str("\n    }");
        }
        
        output.push_str("\n  ]\n}");
        
        output
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> MetricsStats {
        self.global_stats.clone()
    }
    
    /// Metrics'i durdur
    pub fn shutdown(&self) -> Result<(), &'static str> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[METRICS] Shutting down metrics engine");
        
        self.active.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[METRICS] Metrics engine shutdown completed");
        
        Ok(())
    }
}

impl Default for MetricsEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL METRICS ENGINE
// ============================================================================

/// Global metrics engine
static METRICS_ENGINE: MetricsEngine = MetricsEngine::new();

/// Metrics engine'i al
pub fn get_engine() -> &'static MetricsEngine {
    &METRICS_ENGINE
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Metrics'i başlat
pub fn init_metrics() -> Result<(), &'static str> {
    get_engine().init()
}

/// Counter oluştur
pub fn create_counter(name: &str, description: &str) -> Result<u32, &'static str> {
    get_engine().create_counter(name, description)
}

/// Gauge oluştur
pub fn create_gauge(name: &str, description: &str) -> Result<u32, &'static str> {
    get_engine().create_gauge(name, description)
}

/// Histogram oluştur
pub fn create_histogram(name: &str, description: &str) -> Result<u32, &'static str> {
    get_engine().create_histogram(name, description)
}

/// Counter'ı artır
pub fn increment_counter(metric_id: u32, delta: u64) -> Result<(), &'static str> {
    if let Some(metric) = get_engine().get_metric(metric_id) {
        metric.lock().increment(delta);
        Ok(())
    } else {
        Err("Metric not found")
    }
}

/// Gauge'ı ayarla
pub fn set_gauge(metric_id: u32, value: u64) -> Result<(), &'static str> {
    if let Some(metric) = get_engine().get_metric(metric_id) {
        metric.lock().set_gauge(value);
        Ok(())
    } else {
        Err("Metric not found")
    }
}

/// Histogram'a gözlem ekle
pub fn observe_histogram(metric_id: u32, value: u64) -> Result<(), &'static str> {
    if let Some(metric) = get_engine().get_metric(metric_id) {
        metric.lock().observe_histogram(value);
        Ok(())
    } else {
        Err("Metric not found")
    }
}

/// Prometheus formatında export et
pub fn export_prometheus() -> String {
    get_engine().export_prometheus()
}

/// JSON formatında export et
pub fn export_json() -> String {
    get_engine().export_json()
}

/// Metrics testi
pub fn test_metrics() -> Result<(), &'static str> {
    crate::serial_println!("[METRICS] Testing metrics system");
    
    // Metrics'i başlat
    init_metrics()?;
    
    // Test metrikleri oluştur
    let test_counter = create_counter("test_counter", "Test counter")?;
    let test_gauge = create_gauge("test_gauge", "Test gauge")?;
    let test_histogram = create_histogram("test_histogram", "Test histogram")?;
    
    // Counter'ı test et
    increment_counter(test_counter, 1)?;
    increment_counter(test_counter, 5)?;
    increment_counter(test_counter, 10)?;
    
    // Gauge'ı test et
    set_gauge(test_gauge, 42)?;
    set_gauge(test_gauge, 100)?;
    
    // Histogram'ı test et
    observe_histogram(test_histogram, 10)?;
    observe_histogram(test_histogram, 20)?;
    observe_histogram(test_histogram, 30)?;
    observe_histogram(test_histogram, 40)?;
    observe_histogram(test_histogram, 50)?;
    
    // Metrik değerlerini göster
    if let Some(counter) = get_engine().get_metric(test_counter) {
        let value = counter.lock().get_value();
        crate::serial_println!("[METRICS] Counter value: {:?}", value);
    }
    
    if let Some(gauge) = get_engine().get_metric(test_gauge) {
        let value = gauge.lock().get_value();
        crate::serial_println!("[METRICS] Gauge value: {:?}", value);
    }
    
    if let Some(histogram) = get_engine().get_metric(test_histogram) {
        let stats = histogram.lock().get_histogram_stats();
        crate::serial_println!("[METRICS] Histogram stats: {:?}", stats);
    }
    
    // Prometheus export test
    let prometheus_output = export_prometheus();
    crate::serial_println!("[METRICS] Prometheus export:");
    crate::serial_println!("{}", prometheus_output);
    
    // JSON export test
    let json_output = export_json();
    crate::serial_println!("[METRICS] JSON export:");
    crate::serial_println!("{}", json_output);
    
    // İstatistikleri göster
    let stats = get_engine().get_stats();
    crate::serial_println!("[METRICS] Stats:");
    crate::serial_println!("  Total metrics: {}", stats.total_metrics.load(Ordering::SeqCst));
    crate::serial_println!("  Counters: {}", stats.counter_count.load(Ordering::SeqCst));
    crate::serial_println!("  Gauges: {}", stats.gauge_count.load(Ordering::SeqCst));
    crate::serial_println!("  Histograms: {}", stats.histogram_count.load(Ordering::SeqCst));
    crate::serial_println!("  Summaries: {}", stats.summary_count.load(Ordering::SeqCst));
    
    // Metrics'i durdur
    get_engine().shutdown()?;
    
    crate::serial_println!("[METRICS] Metrics test completed");
    
    Ok(())
}
