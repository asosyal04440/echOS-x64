//! # Activity Monitor Application
//!
//! System monitoring app showing CPU, memory, disk, and network usage
//! Process list with resource consumption details
//!
//! Bu modül, macOS Activity Monitor'e benzer bir sistem izleme uygulamasını
//! gerçekleştirir. İşletim sistemlerinde her çalışan program bir "process"
//! (süreç) olarak yönetilir; bu uygulama mevcut süreçleri ve sistem
//! kaynak tüketimini görselleştirir.
//!
//! Temel kavramlar:
//! - **PID**: Process ID — her sürecin işletim sistemi tarafından atanan benzersiz kimliği.
//! - **CPU kullanımı**: İşlemcinin o süreç için ne kadar zaman harcadığının yüzdesi.
//! - **Bellek kullanımı**: Sürecin RAM'de kapladığı alan (bayt cinsinden).
//! - **Uptime**: Sistemin kesintisiz çalışma süresi (saniye cinsinden).

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::sinf;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::Widget;
use crate::gui::Rect;

// ============================================================================
// ACTIVITY MONITOR CONSTANTS
// ============================================================================
// Sabit değerler: UI bileşenlerinin piksel cinsinden boyutları.
// `const` anahtar kelimesi ile derleme zamanında sabit değerler tanımlanır.
// `usize` türü, platform genişliğine (32 veya 64 bit) bağlı işaretsiz tam sayıdır.

/// Sekme çubuğunun piksel cinsinden yüksekliği
pub const TAB_BAR_HEIGHT: usize = 32;

/// Araç çubuğunun piksel cinsinden yüksekliği
pub const TOOLBAR_HEIGHT: usize = 36;

/// Süreç listesinde her satırın yüksekliği
pub const ROW_HEIGHT: usize = 24;

/// Kaynak kullanım grafiğinin yüksekliği
pub const GRAPH_HEIGHT: usize = 100;

/// İstatistik güncelleme aralığı (milisaniye)
pub const UPDATE_INTERVAL: u64 = 1000;

// ============================================================================
// PROCESS INFO
// ============================================================================
// Süreç (process) bilgisini temsil eden veri yapısı.
// Rust'ta `struct` ile ilişkili verileri bir arada gruplayabiliriz.
// `#[derive(Clone, Debug)]` özniteliği, derleyiciye bu struct için
// otomatik olarak Clone (kopyalama) ve Debug (hata ayıklama çıktısı)
// trait'lerini üretmesini söyler.

/// İşletim sistemi tarafından çalıştırılan bir sürece ait bilgiler
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    /// Sürecin benzersiz kimliği (Process ID)
    pub pid: u32,
    /// Sürecin adı (örn. "kernel_task", "Browser")
    pub name: String,
    /// Süreci başlatan kullanıcı
    pub user: String,
    /// CPU kullanım yüzdesi (0.0 - 100.0)
    pub cpu_percent: f32,
    /// Kullanılan bellek miktarı (bayt cinsinden)
    pub memory_bytes: u64,
    /// Toplam belleğe oranla kullanım yüzdesi
    pub memory_percent: f32,
    /// Diskten okunan veri miktarı (bayt)
    pub disk_read: u64,
    /// Diske yazılan veri miktarı (bayt)
    pub disk_write: u64,
    /// Ağdan alınan veri miktarı (bayt)
    pub net_recv: u64,
    /// Ağa gönderilen veri miktarı (bayt)
    pub net_sent: u64,
    /// Süreç içindeki iş parçacığı (thread) sayısı
    pub threads: u32,
    /// Açık ağ portu sayısı
    pub ports: u32,
    /// Sürecin mevcut durumu
    pub state: ProcessState,
    /// Üst sürecin PID'i (parent process)
    pub ppid: u32,
    /// Zamanlayıcı önceliği (düşük = yüksek öncelik Unix'te)
    pub priority: i32,
    /// Sürecin başlama zamanı (UNIX timestamp)
    pub start_time: u64,
}

// Süreç durumlarını temsil eden enum.
// `Copy` trait'i, değerin yığına (stack) kopyalanabildiğini belirtir.
// `PartialEq + Eq` trait'leri == operatörü ile karşılaştırmaya olanak tanır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Running,  // Süreç aktif olarak çalışıyor
    Sleeping, // Eventbeliyor (engellenmiş, ama sonlandırılmamış)
    Idle,     // Boşta bekliyor
    Stopped,  // Dış sinyal ile durdurulmuş (SIGSTOP gibi)
    Zombie,   // Tamamlandı ama üst süreç henüz temizlemedi
}

impl ProcessInfo {
    pub fn new(pid: u32, name: &str) -> Self {
        ProcessInfo {
            pid,
            name: String::from(name),
            user: String::from("user"),
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_percent: 0.0,
            disk_read: 0,
            disk_write: 0,
            net_recv: 0,
            net_sent: 0,
            threads: 1,
            ports: 0,
            state: ProcessState::Running,
            ppid: 0,
            priority: 0,
            start_time: 0,
        }
    }

    // Bayt birimindeki bellek miktarını okunabilir bir stringe dönüştürür.
    // 1 KB = 1024 B, 1 MB = 1024 KB, 1 GB = 1024 MB şeklinde hesaplanır.
    // `as f64` ile tam sayıyı kayan noktalı sayıya dönüştürüp bölme yapılır.
    pub fn format_memory(&self) -> String {
        if self.memory_bytes < 1024 {
            format!("{} B", self.memory_bytes)
        } else if self.memory_bytes < 1024 * 1024 {
            format!("{:.1} KB", self.memory_bytes as f64 / 1024.0)
        } else if self.memory_bytes < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.memory_bytes as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", self.memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    pub fn format_disk(&self) -> String {
        let total = self.disk_read + self.disk_write;
        if total < 1024 {
            format!("{} B", total)
        } else if total < 1024 * 1024 {
            format!("{:.1} KB", total as f64 / 1024.0)
        } else if total < 1024 * 1024 * 1024 {
            format!("{:.1} MB", total as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", total as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
    
    pub fn format_network(&self) -> String {
        let total = self.net_recv + self.net_sent;
        if total < 1024 {
            format!("{} B", total)
        } else if total < 1024 * 1024 {
            format!("{:.1} KB", total as f64 / 1024.0)
        } else if total < 1024 * 1024 * 1024 {
            format!("{:.1} MB", total as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", total as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

// ============================================================================
// SYSTEM STATS
// ============================================================================
// Sistemin anlık kaynak kullanım istatistikleri.
// `Vec<f32>` kullanımı, geçmiş değerleri dinamik dizide saklamamızı sağlar.
// Grafik çizmek için son 60 örnek (yaklaşık 1 dakika) burada tutulur.

/// Sistem genelindeki anlık ve geçmişe ait kaynak kullanım istatistikleri
#[derive(Clone, Debug)]
pub struct SystemStats {
    /// Her çekirdek için ayrı CPU kullanım yüzdesi
    pub cpu_cores: Vec<f32>,
    /// Tüm çekirdeklerin ortalaması olan toplam CPU kullanımı
    pub cpu_total: f32,
    /// Son 60 saniyeye ait CPU kullanım geçmişi (grafik için)
    pub cpu_history: Vec<f32>,
    /// Toplam fiziksel RAM miktarı (bayt)
    pub memory_total: u64,
    /// Şu an kullanımda olan RAM miktarı (bayt)
    pub memory_used: u64,
    /// Bellek baskısı: Normal / Uyarı / Kritik
    pub memory_pressure: MemoryPressure,
    /// Son 60 saniyeye ait bellek kullanım geçmişi
    pub memory_history: Vec<f32>,
    /// Toplam disk alanı (bayt)
    pub disk_total: u64,
    /// Kullanılan disk alanı (bayt)
    pub disk_used: u64,
    /// Anlık disk okuma hızı (bayt/saniye)
    pub disk_read_speed: u64,
    /// Anlık disk yazma hızı (bayt/saniye)
    pub disk_write_speed: u64,
    /// Son 60 saniyeye ait disk kullanım geçmişi
    pub disk_history: Vec<f32>,
    /// Toplam ağdan alınan veri miktarı (bayt)
    pub net_recv: u64,
    /// Toplam ağa gönderilen veri miktarı (bayt)
    pub net_sent: u64,
    /// Anlık indirme hızı (bayt/saniye)
    pub net_recv_speed: u64,
    /// Anlık yükleme hızı (bayt/saniye)
    pub net_send_speed: u64,
    /// Son 60 saniyeye ait ağ trafiği geçmişi (indirme, yükleme) çifti
    pub net_history: Vec<(f32, f32)>,
    /// Sistemin açık kalma süresi (saniye cinsinden)
    pub uptime: u64,
    /// 1, 5 ve 15 dakikalık yük ortalaması (Unix load average)
    pub load_avg: (f32, f32, f32),
    /// Toplam çalışan süreç sayısı
    pub process_count: usize,
    /// Toplam iş parçacığı sayısı
    pub thread_count: u32,
}

// Bellek baskı seviyesi: macOS'un "Memory Pressure" kavramından esinlenilmiştir.
// Sistem daha fazla uygulama için bellek bulamazsa önce uyarı, sonra kritik duruma geçer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryPressure {
    Normal,   // Bellek yeterli
    Warning,  // Bellek azalıyor, bazı önbellek alanları serbest bırakılıyor
    Critical, // Bellek kritik seviyede; sistem yavaşlayabilir
}

impl SystemStats {
    pub fn new() -> Self {
        SystemStats {
            cpu_cores: vec![0.0; 4], // 4 cores
            cpu_total: 0.0,
            cpu_history: Vec::with_capacity(60),
            memory_total: 8 * 1024 * 1024 * 1024, // 8 GB
            memory_used: 2 * 1024 * 1024 * 1024,  // 2 GB
            memory_pressure: MemoryPressure::Normal,
            memory_history: Vec::with_capacity(60),
            disk_total: 256 * 1024 * 1024 * 1024, // 256 GB
            disk_used: 64 * 1024 * 1024 * 1024,  // 64 GB
            disk_read_speed: 0,
            disk_write_speed: 0,
            disk_history: Vec::with_capacity(60),
            net_recv: 0,
            net_sent: 0,
            net_recv_speed: 0,
            net_send_speed: 0,
            net_history: Vec::with_capacity(60),
            uptime: 0,
            load_avg: (0.0, 0.0, 0.0),
            process_count: 0,
            thread_count: 0,
        }
    }
    
    pub fn memory_percent(&self) -> f32 {
        (self.memory_used as f64 / self.memory_total as f64 * 100.0) as f32
    }
    
    pub fn disk_percent(&self) -> f32 {
        (self.disk_used as f64 / self.disk_total as f64 * 100.0) as f32
    }
    
    pub fn format_uptime(&self) -> String {
        let days = self.uptime / 86400;
        let hours = (self.uptime % 86400) / 3600;
        let mins = (self.uptime % 3600) / 60;
        
        if days > 0 {
            format!("{} days, {}:{:02}", days, hours, mins)
        } else if hours > 0 {
            format!("{}:{:02}:00", hours, mins)
        } else {
            format!("{} min", mins)
        }
    }
    
    pub fn format_memory(&self) -> String {
        let used_gb = self.memory_used as f64 / (1024.0 * 1024.0 * 1024.0);
        let total_gb = self.memory_total as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{:.1} / {:.1} GB", used_gb, total_gb)
    }
    
    pub fn format_disk(&self) -> String {
        let used_gb = self.disk_used as f64 / (1024.0 * 1024.0 * 1024.0);
        let total_gb = self.disk_total as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{:.0} / {:.0} GB", used_gb, total_gb)
    }
    
    pub fn format_network_speed(&self) -> String {
        let down = if self.net_recv_speed < 1024 {
            format!("{} B/s", self.net_recv_speed)
        } else if self.net_recv_speed < 1024 * 1024 {
            format!("{:.0} KB/s", self.net_recv_speed as f64 / 1024.0)
        } else {
            format!("{:.1} MB/s", self.net_recv_speed as f64 / (1024.0 * 1024.0))
        };
        
        let up = if self.net_send_speed < 1024 {
            format!("{} B/s", self.net_send_speed)
        } else if self.net_send_speed < 1024 * 1024 {
            format!("{:.0} KB/s", self.net_send_speed as f64 / 1024.0)
        } else {
            format!("{:.1} MB/s", self.net_send_speed as f64 / (1024.0 * 1024.0))
        };
        
        format!("↓{} ↑{}", down, up)
    }
}

// ============================================================================
// MONITOR TAB
// ============================================================================
// Kullanıcının hangi kaynak türünü izleyeceğini seçmesini sağlayan sekme tipi.
// Enum varyantları, farklı izleme kategorilerini temsil eder.

/// Etkinlik izleyicisinin ana sekme türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorTab {
    Cpu,     // İşlemci kullanımı
    Memory,  // Bellek kullanımı
    Energy,  // Enerji tüketimi
    Disk,    // Disk okuma/yazma
    Network, // Ağ trafiği
}

impl MonitorTab {
    pub fn name(&self) -> &'static str {
        match self {
            MonitorTab::Cpu => "CPU",
            MonitorTab::Memory => "Memory",
            MonitorTab::Energy => "Energy",
            MonitorTab::Disk => "Disk",
            MonitorTab::Network => "Network",
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            MonitorTab::Cpu => "⚡",
            MonitorTab::Memory => "💾",
            MonitorTab::Energy => "🔋",
            MonitorTab::Disk => "💿",
            MonitorTab::Network => "📡",
        }
    }
}

// ============================================================================
// ACTIVITY MONITOR WINDOW
// ============================================================================
// Ana pencere yapısı. Rust'ta ownership (sahiplik) modeli gereği,
// bu struct tüm alt verilerin sahibidir. Pencere kapatıldığında
// `Drop` trait'i aracılığıyla tüm alanlar otomatik olarak serbest bırakılır.

/// Etkinlik İzleyici uygulama penceresi
pub struct ActivityMonitor {
    /// Pencerenin ekrandaki konumu ve boyutu
    pub rect: Rect,
    /// Aktif sekme (CPU, Bellek, Enerji vb.)
    pub current_tab: MonitorTab,
    /// Sistem istatistikleri
    pub stats: SystemStats,
    /// Tüm süreçlerin listesi
    pub processes: Vec<ProcessInfo>,
    /// Filtrelenmiş süreçlerin orijinal listedeki indeksleri
    pub filtered_processes: Vec<usize>,
    /// Arama kutusu metni
    pub search_query: String,
    /// Hangi sütuna göre sıralandığı
    pub sort_column: SortColumn,
    /// Artan sıralama mı? (false = azalan)
    pub sort_ascending: bool,
    /// Seçili sürecin PID'i
    pub selected_process: Option<u32>,
    /// Fare imlecinin üzerinde olduğu satır indeksi
    pub hovered_process: Option<usize>,
    /// Kaydırma pozisyonu (kaç satır aşağı kaydırıldı)
    pub scroll_offset: usize,
    /// Sonraki güncellemeye kalan süre (saniye)
    pub update_timer: f32,
    /// Hangi süreçlerin gösterileceği (Tümü, Aktif vb.)
    pub view_mode: ViewMode,
    /// Grafik alanı gösterilsin mi?
    pub show_graph: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Pid,
    Name,
    Cpu,
    Memory,
    Disk,
    Network,
    Threads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    All,
    MyProcesses,
    SystemProcesses,
    Active,
    Inactive,
}

impl ActivityMonitor {
    pub fn new(rect: Rect) -> Self {
        let mut monitor = ActivityMonitor {
            rect,
            current_tab: MonitorTab::Cpu,
            stats: SystemStats::new(),
            processes: Vec::new(),
            filtered_processes: Vec::new(),
            search_query: String::new(),
            sort_column: SortColumn::Cpu,
            sort_ascending: false,
            selected_process: None,
            hovered_process: None,
            scroll_offset: 0,
            update_timer: 0.0,
            view_mode: ViewMode::All,
            show_graph: true,
        };
        
        monitor.init_processes();
        monitor.filter_and_sort();
        
        monitor
    }
    
    fn init_processes(&mut self) {
        // Add sample processes
        let mut p1 = ProcessInfo::new(1, "kernel_task");
        p1.cpu_percent = 2.5;
        p1.memory_bytes = 512 * 1024 * 1024;
        p1.memory_percent = 6.25;
        p1.threads = 64;
        p1.state = ProcessState::Running;
        self.processes.push(p1);
        
        let mut p2 = ProcessInfo::new(100, "WindowServer");
        p2.cpu_percent = 8.2;
        p2.memory_bytes = 256 * 1024 * 1024;
        p2.memory_percent = 3.125;
        p2.threads = 12;
        p2.state = ProcessState::Running;
        self.processes.push(p2);
        
        let mut p3 = ProcessInfo::new(200, "Finder");
        p3.cpu_percent = 1.5;
        p3.memory_bytes = 128 * 1024 * 1024;
        p3.memory_percent = 1.56;
        p3.threads = 8;
        p3.state = ProcessState::Running;
        self.processes.push(p3);
        
        let mut p4 = ProcessInfo::new(300, "Terminal");
        p4.cpu_percent = 0.8;
        p4.memory_bytes = 64 * 1024 * 1024;
        p4.memory_percent = 0.78;
        p4.threads = 4;
        p4.state = ProcessState::Running;
        self.processes.push(p4);
        
        let mut p5 = ProcessInfo::new(400, "Browser");
        p5.cpu_percent = 15.3;
        p5.memory_bytes = 1024 * 1024 * 1024;
        p5.memory_percent = 12.5;
        p5.threads = 24;
        p5.disk_read = 50 * 1024 * 1024;
        p5.disk_write = 20 * 1024 * 1024;
        p5.net_recv = 100 * 1024 * 1024;
        p5.net_sent = 50 * 1024 * 1024;
        p5.state = ProcessState::Running;
        self.processes.push(p5);
        
        let mut p6 = ProcessInfo::new(500, "Music Player");
        p6.cpu_percent = 2.1;
        p6.memory_bytes = 200 * 1024 * 1024;
        p6.memory_percent = 2.44;
        p6.threads = 6;
        p6.state = ProcessState::Running;
        self.processes.push(p6);
        
        let mut p7 = ProcessInfo::new(600, "Text Editor");
        p7.cpu_percent = 0.3;
        p7.memory_bytes = 80 * 1024 * 1024;
        p7.memory_percent = 0.98;
        p7.threads = 3;
        p7.state = ProcessState::Running;
        self.processes.push(p7);
        
        let mut p8 = ProcessInfo::new(700, "System Preferences");
        p8.cpu_percent = 0.1;
        p8.memory_bytes = 40 * 1024 * 1024;
        p8.memory_percent = 0.49;
        p8.threads = 2;
        p8.state = ProcessState::Idle;
        self.processes.push(p8);
        
        self.stats.process_count = self.processes.len();
        self.stats.thread_count = self.processes.iter().map(|p| p.threads).sum();
    }
    
    fn filter_and_sort(&mut self) {
        // Filter
        self.filtered_processes.clear();
        
        for (i, p) in self.processes.iter().enumerate() {
            // Search filter
            if !self.search_query.is_empty() {
                let query = self.search_query.to_lowercase();
                if !p.name.to_lowercase().contains(&query) {
                    continue;
                }
            }
            
            // View mode filter
            match self.view_mode {
                ViewMode::Active => {
                    if p.cpu_percent < 0.1 && p.memory_percent < 0.1 {
                        continue;
                    }
                }
                ViewMode::Inactive => {
                    if p.cpu_percent >= 0.1 || p.memory_percent >= 0.1 {
                        continue;
                    }
                }
                _ => {}
            }
            
            self.filtered_processes.push(i);
        }
        
        // Sort
        let sort_col = self.sort_column;
        let ascending = self.sort_ascending;
        
        self.filtered_processes.sort_by(|a, b| {
            let pa = &self.processes[*a];
            let pb = &self.processes[*b];
            
            let cmp = match sort_col {
                SortColumn::Pid => pa.pid.cmp(&pb.pid),
                SortColumn::Name => pa.name.to_lowercase().cmp(&pb.name.to_lowercase()),
                SortColumn::Cpu => pa.cpu_percent.partial_cmp(&pb.cpu_percent).unwrap_or(core::cmp::Ordering::Equal),
                SortColumn::Memory => pa.memory_bytes.cmp(&pb.memory_bytes),
                SortColumn::Disk => (pa.disk_read + pa.disk_write).cmp(&(pb.disk_read + pb.disk_write)),
                SortColumn::Network => (pa.net_recv + pa.net_sent).cmp(&(pb.net_recv + pb.net_sent)),
                SortColumn::Threads => pa.threads.cmp(&pb.threads),
            };
            
            if ascending { cmp } else { cmp.reverse() }
        });
    }
    
    pub fn set_tab(&mut self, tab: MonitorTab) {
        self.current_tab = tab;
    }
    
    pub fn set_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = column;
            self.sort_ascending = false;
        }
        self.filter_and_sort();
    }
    
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.filter_and_sort();
    }
    
    /// İstatistikleri güncelle - her saniyede bir çağrılır.
    /// `dt` (delta time): Son güncellemeden bu yana geçen süre (saniye).
    /// `sinf` fonksiyonu, simüle edilmiş dalgalı CPU kullanımı üretmek için kullanılır.
    pub fn update(&mut self, dt: f32) {
        self.update_timer += dt;
        
        if self.update_timer >= 1.0 {
            self.update_timer = 0.0;
            
            // Update system stats
            self.stats.uptime += 1;
            
            // Simulate CPU fluctuation
            for i in 0..self.stats.cpu_cores.len() {
                self.stats.cpu_cores[i] = (self.stats.cpu_cores[i] + 5.0 * sinf(self.stats.uptime as f32 / 2.0 + i as f32)) % 100.0;
                self.stats.cpu_cores[i] = self.stats.cpu_cores[i].max(5.0).min(95.0);
            }
            self.stats.cpu_total = self.stats.cpu_cores.iter().sum::<f32>() / self.stats.cpu_cores.len() as f32;
            
            // Update history
            self.stats.cpu_history.push(self.stats.cpu_total);
            if self.stats.cpu_history.len() > 60 {
                self.stats.cpu_history.remove(0);
            }
            
            self.stats.memory_history.push(self.stats.memory_percent());
            if self.stats.memory_history.len() > 60 {
                self.stats.memory_history.remove(0);
            }
            
            // Simulate network activity
            self.stats.net_recv_speed = (50.0 + 100.0 * sinf(self.stats.uptime as f32 / 5.0)) as u64 * 1024;
            self.stats.net_send_speed = (20.0 + 50.0 * sinf(self.stats.uptime as f32 / 3.0)) as u64 * 1024;
            self.stats.net_recv += self.stats.net_recv_speed;
            self.stats.net_sent += self.stats.net_send_speed;
            
            self.stats.net_history.push((
                self.stats.net_recv_speed as f32 / 1024.0 / 1024.0,
                self.stats.net_send_speed as f32 / 1024.0 / 1024.0,
            ));
            if self.stats.net_history.len() > 60 {
                self.stats.net_history.remove(0);
            }
            
            // Update process stats
            for p in &mut self.processes {
                p.cpu_percent = (p.cpu_percent + 2.0 * sinf(self.stats.uptime as f32 / 3.0 + p.pid as f32)) % 30.0;
                p.cpu_percent = p.cpu_percent.max(0.0);
            }
        }
    }
    
    /// Draw Activity Monitor
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let w = self.rect.width as usize;
        let h = self.rect.height as usize;
        
        // Background
        fb.draw_rect(x, y, w, h, Theme::WINDOW_BG.to_u32());
        fb.draw_rect_outline(x, y, w, h, Theme::BORDER.to_u32());
        
        // Toolbar
        fb.draw_rect(x, y, w, TOOLBAR_HEIGHT, Theme::TOOLBAR_BG.to_u32());
        self.draw_toolbar(fb, x, y, w);
        
        // Tab bar
        let tab_y = y + TOOLBAR_HEIGHT;
        fb.draw_rect(x, tab_y, w, TAB_BAR_HEIGHT, Theme::SIDEBAR_BG.to_u32());
        self.draw_tabs(fb, x, tab_y, w);
        
        // Content
        let content_y = tab_y + TAB_BAR_HEIGHT;
        let content_h = h - TOOLBAR_HEIGHT - TAB_BAR_HEIGHT;
        
        // Graph area
        if self.show_graph {
            self.draw_graph(fb, x, content_y, w, GRAPH_HEIGHT);
        }
        
        // Process list
        let list_y = if self.show_graph { content_y + GRAPH_HEIGHT + 8 } else { content_y };
        let list_h = if self.show_graph { content_h - GRAPH_HEIGHT - 8 } else { content_h };
        
        self.draw_process_list(fb, x, list_y, w, list_h);
    }
    
    fn draw_toolbar(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        // View mode dropdown
        let view_modes = ["All", "My Processes", "System", "Active"];
        let mut btn_x = x + 8;
        
        for (i, mode) in view_modes.iter().enumerate() {
            let is_active = match (self.view_mode, i) {
                (ViewMode::All, 0) => true,
                (ViewMode::MyProcesses, 1) => true,
                (ViewMode::SystemProcesses, 2) => true,
                (ViewMode::Active, 3) => true,
                _ => false,
            };
            
            let bg = if is_active { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            let text_color = if is_active { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            
            fb.draw_rect(btn_x, y + 4, mode.len() * 8 + 16, 28, bg);
            fb.draw_string(btn_x + 8, y + 8, mode, text_color);
            
            btn_x += mode.len() * 8 + 20;
        }
        
        // Search field
        let search_x = x + w - 180;
        fb.draw_rect(search_x, y + 4, 160, 28, Theme::SIDEBAR_BG.to_u32());
        fb.draw_string(search_x + 8, y + 8, "🔍", Theme::TEXT_SECONDARY.to_u32());
        
        if self.search_query.is_empty() {
            fb.draw_string(search_x + 28, y + 8, "Search", Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(search_x + 28, y + 8, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
        }
    }
    
    fn draw_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize) {
        let tabs = [MonitorTab::Cpu, MonitorTab::Memory, MonitorTab::Energy, MonitorTab::Disk, MonitorTab::Network];
        let mut tab_x = x + 8;
        
        for tab in tabs {
            let is_active = self.current_tab == tab;
            let bg = if is_active { Theme::WINDOW_BG.to_u32() } else { Theme::SIDEBAR_BG.to_u32() };
            
            fb.draw_rect(tab_x, y, 80, TAB_BAR_HEIGHT, bg);
            fb.draw_string(tab_x + 8, y + 8, tab.icon(), Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(tab_x + 28, y + 8, tab.name(), Theme::TEXT_PRIMARY.to_u32());
            
            tab_x += 84;
        }
    }
    
    fn draw_graph(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Graph background
        fb.draw_rect(x, y, w, h, Theme::SIDEBAR_BG.to_u32());
        
        // Title
        let title = match self.current_tab {
            MonitorTab::Cpu => format!("CPU: {:.1}%", self.stats.cpu_total),
            MonitorTab::Memory => format!("Memory: {:.1}%", self.stats.memory_percent()),
            MonitorTab::Disk => format!("Disk: {:.1}%", self.stats.disk_percent()),
            MonitorTab::Network => self.stats.format_network_speed(),
            MonitorTab::Energy => String::from("Energy Impact"),
        };
        fb.draw_string(x + 8, y + 4, &title, Theme::TEXT_PRIMARY.to_u32());
        
        // Draw graph based on current tab
        let graph_y = y + 24;
        let graph_h = h - 32;
        
        match self.current_tab {
            MonitorTab::Cpu => {
                self.draw_line_graph(fb, x + 8, graph_y, w - 16, graph_h, &self.stats.cpu_history, 0xFF00B894);
            }
            MonitorTab::Memory => {
                self.draw_line_graph(fb, x + 8, graph_y, w - 16, graph_h, &self.stats.memory_history, 0xFF0984E3);
            }
            MonitorTab::Network => {
                self.draw_network_graph(fb, x + 8, graph_y, w - 16, graph_h);
            }
            _ => {
                // Placeholder for other tabs
                fb.draw_string(x + w / 2 - 40, graph_y + graph_h / 2, "Graph placeholder", Theme::TEXT_SECONDARY.to_u32());
            }
        }
        
        // CPU cores (for CPU tab)
        if self.current_tab == MonitorTab::Cpu {
            let core_w = 60;
            let core_h = 40;
            let mut core_x = x + w - 280;
            
            for (i, &usage) in self.stats.cpu_cores.iter().enumerate() {
                self.draw_core_graph(fb, core_x, graph_y, core_w - 8, core_h, i, usage);
                core_x += core_w;
            }
        }
    }
    
    fn draw_line_graph(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, data: &[f32], color: u32) {
        if data.is_empty() {
            return;
        }
        
        // Draw grid lines
        for i in 0..5 {
            let line_y = y + i * h / 4;
            fb.draw_rect(x, line_y, w, 1, Theme::BORDER.to_u32());
            
            let label = format!("{}%", 100 - i * 25);
            fb.draw_string(x, line_y + 2, &label, Theme::TEXT_SECONDARY.to_u32());
        }
        
        // Draw line
        let step_x = w as f32 / (data.len() - 1).max(1) as f32;
        
        for (i, &value) in data.iter().enumerate() {
            let px = x + (i as f32 * step_x) as usize;
            let py = y + h - (value / 100.0 * h as f32) as usize;
            
            if i > 0 {
                let prev_value = data[i - 1];
                let prev_px = x + ((i - 1) as f32 * step_x) as usize;
                let prev_py = y + h - (prev_value / 100.0 * h as f32) as usize;
                
                // Draw line segment
                for t in 0..100 {
                    let t = t as f32 / 100.0;
                    let lx = (prev_px as f32 + (px as f32 - prev_px as f32) * t) as usize;
                    let ly = (prev_py as f32 + (py as f32 - prev_py as f32) * t) as usize;
                    
                    if lx < x + w && ly < y + h {
                        fb.plot_pixel(lx, ly, color);
                    }
                }
            }
            
            // Draw point
            if px < x + w && py < y + h {
                fb.plot_pixel(px, py, color);
            }
        }
        
        // Fill area under line
        for (i, &value) in data.iter().enumerate() {
            let px = x + (i as f32 * step_x) as usize;
            let py = y + h - (value / 100.0 * h as f32) as usize;
            
            for fill_y in py..y + h {
                if px < x + w && fill_y < y + h {
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(fill_y * fb.pixels_per_scan_line + px) };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = Self::blend_color(bg, color, 0.2); }
                }
            }
        }
    }
    
    fn draw_network_graph(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        if self.stats.net_history.is_empty() {
            return;
        }
        
        // Draw two lines for download/upload
        let down_data: Vec<f32> = self.stats.net_history.iter().map(|(d, _)| *d).collect();
        let up_data: Vec<f32> = self.stats.net_history.iter().map(|(_, u)| *u).collect();
        
        self.draw_line_graph(fb, x, y, w, h, &down_data, 0xFF00B894);
        self.draw_line_graph(fb, x, y, w, h, &up_data, 0xFFE17055);
    }
    
    fn draw_core_graph(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize, core: usize, usage: f32) {
        // Core label
        let label = format!("Core {}", core);
        fb.draw_string(x, y, &label, Theme::TEXT_SECONDARY.to_u32());
        
        // Usage bar
        let bar_y = y + 16;
        let bar_h = h - 16;
        let filled_h = (usage / 100.0 * bar_h as f32) as usize;
        
        fb.draw_rect(x, bar_y, w, bar_h, Theme::BORDER.to_u32());
        
        // Color based on usage
        let color = if usage < 50.0 { 0xFF00B894 }
                    else if usage < 80.0 { 0xFFFDCB6E }
                    else { 0xFFE17055 };
        
        fb.draw_rect(x, bar_y + bar_h - filled_h, w, filled_h, color);
        
        // Percentage
        let pct = format!("{:.0}%", usage);
        fb.draw_string(x, bar_y + bar_h + 2, &pct, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_process_list(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // Column headers
        let header_h = 24;
        fb.draw_rect(x, y, w, header_h, Theme::TOOLBAR_BG.to_u32());
        
        let columns = [
            ("PID", 60, SortColumn::Pid),
            ("Process Name", 200, SortColumn::Name),
            ("CPU", 80, SortColumn::Cpu),
            ("Memory", 100, SortColumn::Memory),
            ("Threads", 60, SortColumn::Threads),
        ];
        
        let mut col_x = x + 8;
        
        for (name, width, col) in columns {
            let is_sorted = self.sort_column == col;
            let color = if is_sorted { Theme::ACCENT_PRIMARY.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
            
            let arrow = if is_sorted {
                if self.sort_ascending { " ▴" } else { " ▾" }
            } else {
                ""
            };
            
            fb.draw_string(col_x, y + 4, name, color);
            if !arrow.is_empty() {
                fb.draw_string(col_x + name.len() * 8, y + 4, arrow, color);
            }
            
            col_x += width;
        }
        
        // Process rows
        let row_y = y + header_h;
        let visible_rows = (h - header_h) / ROW_HEIGHT;
        
        for (i, &proc_idx) in self.filtered_processes.iter().skip(self.scroll_offset).take(visible_rows).enumerate() {
            let proc = &self.processes[proc_idx];
            let row_y = row_y + i * ROW_HEIGHT;
            
            let is_selected = self.selected_process == Some(proc.pid);
            let is_hovered = self.hovered_process == Some(self.scroll_offset + i);
            
            let bg = if is_selected { Theme::ACCENT_PRIMARY.to_u32() }
                     else if is_hovered { Theme::LIST_ITEM_HOVER.to_u32() }
                     else { Theme::WINDOW_BG.to_u32() };
            
            fb.draw_rect(x, row_y, w, ROW_HEIGHT, bg);
            
            let text_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            let secondary_color = if is_selected { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_SECONDARY.to_u32() };
            
            // PID
            fb.draw_string(x + 8, row_y + 4, &format!("{}", proc.pid), text_color);
            
            // Name
            let name = if proc.name.len() > 20 { format!("{}...", &proc.name[..17]) } else { proc.name.clone() };
            fb.draw_string(x + 68, row_y + 4, &name, text_color);
            
            // CPU
            let cpu_color = if proc.cpu_percent > 50.0 { Theme::ERROR.to_u32() }
                           else if proc.cpu_percent > 20.0 { Theme::ACCENT_WARNING.to_u32() }
                           else { text_color };
            fb.draw_string(x + 268, row_y + 4, &format!("{:.1}%", proc.cpu_percent), cpu_color);
            
            // Memory
            fb.draw_string(x + 348, row_y + 4, &proc.format_memory(), text_color);
            
            // Threads
            fb.draw_string(x + 448, row_y + 4, &format!("{}", proc.threads), secondary_color);
        }
        
        // Empty state
        if self.filtered_processes.is_empty() {
            fb.draw_string(x + w / 2 - 40, row_y + h / 2, "No processes found", Theme::TEXT_SECONDARY.to_u32());
        }
    }
    
    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;
        
        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;
        
        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;
        
        (r << 16) | (g << 8) | b
    }
    
    /// Handle click
    pub fn on_click(&mut self, mx: i32, my: i32) -> MonitorAction {
        let x = self.rect.x;
        let y = self.rect.y;
        let w = self.rect.width;
        
        // View mode buttons
        if my >= y + 4 && my < y + 32 {
            let view_modes = [ViewMode::All, ViewMode::MyProcesses, ViewMode::SystemProcesses, ViewMode::Active];
            let mut btn_x = x + 8;
            
            let labels = ["All", "My Processes", "System", "Active"];
            for (i, label) in labels.iter().enumerate() {
                let btn_w = (label.len() * 8 + 16) as i32;
                if mx >= btn_x && mx < btn_x + btn_w {
                    self.set_view_mode(view_modes[i]);
                    return MonitorAction::None;
                }
                btn_x += btn_w + 4;
            }
        }
        
        // Tabs
        let tab_y = y + TOOLBAR_HEIGHT as i32;
        if my >= tab_y && my < tab_y + TAB_BAR_HEIGHT as i32 {
            let tabs = [MonitorTab::Cpu, MonitorTab::Memory, MonitorTab::Energy, MonitorTab::Disk, MonitorTab::Network];
            let mut tab_x = x + 8;
            
            for tab in tabs {
                if mx >= tab_x && mx < tab_x + 80 {
                    self.set_tab(tab);
                    return MonitorAction::None;
                }
                tab_x += 84;
            }
        }
        
        // Column headers
        let header_y = tab_y + TAB_BAR_HEIGHT as i32 + if self.show_graph { GRAPH_HEIGHT as i32 + 8 } else { 0 };
        if my >= header_y && my < header_y + 24 {
            let columns = [
                (SortColumn::Pid, 60),
                (SortColumn::Name, 200),
                (SortColumn::Cpu, 80),
                (SortColumn::Memory, 100),
                (SortColumn::Threads, 60),
            ];
            
            let mut col_x = x + 8;
            for (col, width) in columns {
                if mx >= col_x && mx < col_x + width {
                    self.set_sort(col);
                    return MonitorAction::None;
                }
                col_x += width;
            }
        }
        
        // Process rows
        let row_y = header_y + 24;
        let visible_rows = ((self.rect.height as usize) - TOOLBAR_HEIGHT - TAB_BAR_HEIGHT - 24 - if self.show_graph { GRAPH_HEIGHT + 8 } else { 0 }) / ROW_HEIGHT;
        
        for i in 0..visible_rows {
            let idx = self.scroll_offset + i;
            if idx >= self.filtered_processes.len() {
                break;
            }
            
            let proc_y = row_y + (i * ROW_HEIGHT) as i32;
            if my >= proc_y && my < proc_y + ROW_HEIGHT as i32 {
                let proc = &self.processes[self.filtered_processes[idx]];
                self.selected_process = Some(proc.pid);
                return MonitorAction::ProcessSelected(proc.pid);
            }
        }
        
        MonitorAction::None
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> MonitorAction {
        match c {
            '\x08' => { // Backspace
                self.search_query.pop();
                self.filter_and_sort();
            }
            '\x1b' => { // Escape
                self.search_query.clear();
                self.filter_and_sort();
            }
            _ if !c.is_control() => {
                self.search_query.push(c);
                self.filter_and_sort();
            }
            _ => {}
        }
        
        MonitorAction::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.rect.width = width as i32;
        self.rect.height = height as i32;
    }
}

/// Monitor actions
#[derive(Clone, Debug)]
pub enum MonitorAction {
    None,
    ProcessSelected(u32),
    KillProcess(u32),
    InspectProcess(u32),
}

// ============================================================================
// GLOBAL ACTIVITY MONITOR
// ============================================================================
// `lazy_static!` makrosu, global statik değişkenleri "ilk kullanımda"
// başlatmak için kullanılır. `Mutex<T>` ise çok iş parçacıklı erişimi
// güvenli hale getirir; kilidi almadan iç veriye ulaşılamaz.
// `spin::Mutex`, standart kütüphane olmadan (no_std) kullanılabilen
// bir döngüsel kilit (spinlock) implementasyonudur.

lazy_static::lazy_static! {
    static ref MONITOR: Mutex<ActivityMonitor> = Mutex::new(ActivityMonitor::new(Rect {
        x: 100,
        y: 100,
        width: 800,
        height: 600,
    }));
}

/// Initialize Activity Monitor
pub fn init() {
    crate::serial_println!("[GUI] Activity Monitor initialized");
}

/// Get Activity Monitor
pub fn get_monitor() -> &'static Mutex<ActivityMonitor> {
    &MONITOR
}
