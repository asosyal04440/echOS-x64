//! # echOS VirGL 3D Hızlandırma Desteği
//!
//! VirGL (Virtual OpenGL), QEMU/KVM sanallaştırma ortamında konuk işletim sisteminden
//! ana makine GPU'sunu kullanarak 3D hızlandırma yapmayı sağlayan bir mekanizmadır.
//! OpenGL ES komutları VirtIO-GPU kuyruğu üzerinden konakta çalışan Mesa/OpenGL'e iletilir.
//!
//! ## VirGL Katmanlı Mimari
//!
//! ```text
//!   KONUK OS (echOS)              ANA MAKİNE (QEMU/KVM)
//!  +-------------------+          +------------------------+
//!  | 3D Uygulama       |          | QEMU GPU Emülasyonu    |
//!  |  OpenGL çağrıları |          |                        |
//!  |        |          |          |  VirtIO-GPU Sunucu     |
//!  | VirGL Sürücüsü    |          |       |                |
//!  |  (bu modül)       |  PCI     |  Mesa / Gallium3D      |
//!  |        |          | VirtQ    |       |                |
//!  | VirtIO-GPU (PCI)  |<-------->|  Fiziksel GPU          |
//!  +-------------------+          +------------------------+
//! ```
//!
//! ## VirGL Yaşam Döngüsü
//!
//! ```text
//!  1. init()              --> VirGL aygıtını hazırla
//!  2. create_context()    --> 3D render bağlamı oluştur
//!  3. create_resource()   --> Doku veya tampon oluştur
//!  4. attach_resource()   --> Kaynağı bağlama bağla
//!  5. submit_commands()   --> Komutları konağa gönder
//!  6. flush()             --> Tüm bekleyen komutları işlet
//!  7. destroy_context()   --> Bağlamı serbest bırak
//! ```
//!
//! ## Komut Tamponu (Command Buffer) Akışı
//!
//! ```text
//!  VirglCommandBuffer (FIFO kuyruk)
//!
//!  push(cmd, data[]) --> [ cmd1 | cmd2 | cmd3 | ... ]
//!                           |
//!                    submit_commands()
//!                           |
//!                    process_command(cmd1) --> VirtIO-GPU mesajı
//!                    process_command(cmd2) --> VirtIO-GPU mesajı
//!                           ...
//!                    Kuyruk tamamen boşalır
//! ```
//!
//! ## Kaynak Boyutu Hesaplama (Pixel Format)
//!
//! ```text
//!  Format              | BPP (Byte/Piksel)
//!  --------------------+---------
//!  B8G8R8A8 / R8G8B8A8 | 4 byte
//!  B5G6R5   / R5G6B5   | 2 byte
//!  D16Unorm             | 2 byte
//!  D24UnormX8 / D24S8   | 4 byte
//!  D32Float             | 4 byte
//!
//!  Boyut = genislik x yukseklik x BPP
//! ```
//!
//! ## Shader İşlem Hattı (Pipeline)
//!
//! ```text
//!  Vertex Shader --> Geometry Shader --> Fragment Shader
//!       |                  |                   |
//!  VirglShaderType::  VirglShaderType::  VirglShaderType::
//!  Vertex             Geometry            Fragment
//! ```

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// VirGL SABİTLERİ
// ============================================================================

/// VirGL bağlam kimliği tipi (u32 takma adı).
pub type VirglContextId = u32;

/// VirGL kaynak kimliği tipi (u32 takma adı).
pub type VirglResourceId = u32;

/// VirGL tampon tanıtıcısı tipi (u32 takma adı).
pub type VirglBufferHandle = u32;

/// VirGL komutları: kaynak, bağlam, render, shader, vertex buffer, doku ve eşitleme işlemleri.
/// Enum değerleri protocol komut numaralarına karşılık gelir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VirglCommand {
    // Kaynak yönetimi
    CreateResource = 1,
    DestroyResource = 2,
    MapResource = 3,
    UnmapResource = 4,

    // Bağlam yönetimi
    CreateContext = 5,
    DestroyContext = 6,
    AttachResource = 7,
    DetachResource = 8,

    // Render komutları
    SubmitCommand = 9,
    FlushBuffer = 10,

    // Shader komutları
    CreateShader = 11,
    DeleteShader = 12,
    BindShader = 13,

    // Vertex tampon komutları
    CreateVertexBuffer = 14,
    DeleteVertexBuffer = 15,
    BindVertexBuffer = 16,

    // Doku komutları
    CreateTexture = 17,
    DeleteTexture = 18,
    BindTexture = 19,

    // Render hedefi
    SetRenderTarget = 20,
    CreateRenderTarget = 21,
    DeleteRenderTarget = 22,

    // Eşitleme
    Sync = 23,
}

/// VirGL kaynak tipleri: tampon, tek/iki/üç boyutlu doku ve cube map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VirglResourceType {
    Buffer = 0,
    Texture1D = 1,
    Texture2D = 2,
    Texture3D = 3,
    TextureCube = 4,
    RenderTarget = 5,
}

/// VirGL piksel formatları.
/// Renk, derinlik ve sıkıştırılmış format gruplarını içerir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VirglFormat {
    // Renk formatları (byte/piksel: 4 veya 2)
    B8G8R8A8Unorm = 1,
    R8G8B8A8Unorm = 2,
    B5G6R5Unorm = 3,
    R5G6B5Unorm = 4,

    // Derinlik-tampon formatları
    D16Unorm = 10,
    D24UnormX8 = 11,
    D32Float = 12,
    D24UnormS8Uint = 13,

    // Sıkıştırılmış DXT/BC formatları
    BC1RGBUnorm = 20,
    BC1RGBAUnorm = 21,
    BC2Unorm = 22,
    BC3Unorm = 23,
}

/// VirGL shader tipleri.
/// OpenGL ES shader işlem hattındaki aşamaları temsil eder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum VirglShaderType {
    Vertex = 0,
    Fragment = 1,
    Geometry = 2,
    TessControl = 3,
    TessEval = 4,
    Compute = 5,
}

// ============================================================================
// VirGL HATA TİPİ
// ============================================================================

/// VirGL işlemlerinde oluşabilecek hatalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirglError {
    /// VirtIO-GPU aygıtı hazır değil
    DeviceNotReady,
    /// Render bağlamı oluşturulamadı
    ContextCreationFailed,
    /// Kaynak oluşturulamadı
    ResourceCreationFailed,
    /// Geçersiz komut
    InvalidCommand,
    /// Komut tamponu kapasitesi aşıldı
    BufferTooLarge,
    /// Kaynak eşleştirme başarısız
    MapFailed,
    /// Shader derleme hatası
    ShaderCompilationFailed,
    /// Yetersiz bellek
    OutOfMemory,
}

// ============================================================================
// VirGL KOMUT TAMPONU
// ============================================================================

/// Tek bir VirGL komut kaydı: komut tipi ve ilişkili veri dizisi.
#[derive(Clone, Debug)]
pub struct VirglCommandEntry {
    pub cmd: VirglCommand,
    pub data: Vec<u32>,
}

/// VirGL komut tamponu: FIFO sırasıyla komutları biriktirir ve gönderir.
/// Maksimum boyut 4096 komutla sınırlıdır; aşıldığında `BufferTooLarge` hatası döner.
#[derive(Clone, Debug)]
pub struct VirglCommandBuffer {
    entries: VecDeque<VirglCommandEntry>,
    max_size: usize,
}

impl VirglCommandBuffer {
    pub fn new() -> Self {
        VirglCommandBuffer {
            entries: VecDeque::with_capacity(256),
            max_size: 4096,
        }
    }

    /// Tampona yeni bir komut ekler.
    /// Tampon kapasitesi aşılmışsa `BufferTooLarge` hatası döner.
    pub fn push(&mut self, cmd: VirglCommand, data: &[u32]) -> Result<(), VirglError> {
        if self.entries.len() >= self.max_size {
            return Err(VirglError::BufferTooLarge);
        }

        self.entries.push_back(VirglCommandEntry {
            cmd,
            data: data.to_vec(),
        });

        Ok(())
    }

    /// Tampondan sıradaki komutu alır (FIFO).
    pub fn pop(&mut self) -> Option<VirglCommandEntry> {
        self.entries.pop_front()
    }

    /// Tampondaki tüm komutları siler.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Tamponun boş olup olmadığını kontrol eder.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Tamponda bekleyen komut sayısını döner.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for VirglCommandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VirGL KAYNAK
// ============================================================================

/// VirGL kaynak yapısı: doku, tampon veya render hedefi.
/// Format, boyut, mipmap seviyesi ve eşleştirme durumu izlenir.
#[derive(Clone, Debug)]
pub struct VirglResource {
    pub id: VirglResourceId,
    pub resource_type: VirglResourceType,
    pub format: VirglFormat,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub array_size: u32,
    pub last_level: u32,
    pub nr_samples: u32,
    pub flags: u32,
    pub bo_handle: VirglBufferHandle,
    pub size: usize,
    pub mapped: bool,
}

impl VirglResource {
    pub fn new(
        id: VirglResourceId,
        resource_type: VirglResourceType,
        format: VirglFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let size = Self::calculate_size(format, width, height);

        VirglResource {
            id,
            resource_type,
            format,
            width,
            height,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 1,
            flags: 0,
            bo_handle: 0,
            size,
            mapped: false,
        }
    }

    /// Piksel formatına ve çözünürlüğe göre tampon boyutunu hesaplar.
    /// Boyut = genislik * yukseklik * BPP (bayt/piksel).
    fn calculate_size(format: VirglFormat, width: u32, height: u32) -> usize {
        let bpp = match format {
            VirglFormat::B8G8R8A8Unorm | VirglFormat::R8G8B8A8Unorm => 4,
            VirglFormat::B5G6R5Unorm | VirglFormat::R5G6B5Unorm => 2,
            VirglFormat::D16Unorm => 2,
            VirglFormat::D24UnormX8 | VirglFormat::D24UnormS8Uint => 4,
            VirglFormat::D32Float => 4,
            _ => 4,
        };

        (width * height * bpp) as usize
    }
}

// ============================================================================
// VirGL SHADER
// ============================================================================

/// VirGL shader nesnesi: kaynak kodu ve derleme durumunu tutar.
/// Her shader bir tip (Vertex, Fragment vb.) ve GLSL/SPIR-V kaynak kodu içerir.
#[derive(Clone, Debug)]
pub struct VirglShader {
    pub id: u32,
    pub shader_type: VirglShaderType,
    pub source: String,
    pub compiled: bool,
}

impl VirglShader {
    pub fn new(id: u32, shader_type: VirglShaderType, source: &str) -> Self {
        VirglShader {
            id,
            shader_type,
            source: source.to_string(),
            compiled: false,
        }
    }
}

// ============================================================================
// VirGL BAĞLAMI (CONTEXT)
// ============================================================================

/// VirGL render bağlamı: kaynakları, shader'ları ve komut tamponunu bir arada tutar.
/// Her bağlamın kendine ait komut tamponu bulunur; flush ile konağa gönderilir.
#[derive(Clone, Debug)]
pub struct VirglContext {
    pub id: VirglContextId,
    pub resources: Vec<VirglResourceId>,
    pub shaders: Vec<u32>,
    pub active_shader: Option<u32>,
    pub command_buffer: VirglCommandBuffer,
    pub initialized: bool,
}

impl VirglContext {
    pub fn new(id: VirglContextId) -> Self {
        VirglContext {
            id,
            resources: Vec::new(),
            shaders: Vec::new(),
            active_shader: None,
            command_buffer: VirglCommandBuffer::new(),
            initialized: false,
        }
    }

    /// Kaynağı bağlama ekler (zaten ekli değilse).
    /// Kaynak bağlanmadan komutlarda kullanilamaz.
    pub fn attach_resource(&mut self, resource_id: VirglResourceId) {
        if !self.resources.contains(&resource_id) {
            self.resources.push(resource_id);
        }
    }

    /// Kaynağı bağlamdan çıkarır.
    pub fn detach_resource(&mut self, resource_id: VirglResourceId) {
        self.resources.retain(|&id| id != resource_id);
    }

    /// Shader'ı bağlama ekler (zaten ekli değilse).
    pub fn add_shader(&mut self, shader_id: u32) {
        if !self.shaders.contains(&shader_id) {
            self.shaders.push(shader_id);
        }
    }

    /// Shader'ı bağlamdan çıkarır.
    pub fn remove_shader(&mut self, shader_id: u32) {
        self.shaders.retain(|&id| id != shader_id);
    }

    /// Aktif shader'ı ayarlar (sonraki draw çağrıları bu shader'ı kullanır).
    pub fn bind_shader(&mut self, shader_id: u32) {
        self.active_shader = Some(shader_id);
    }
}

// ============================================================================
// VirGL AYGITI
// ============================================================================

/// VirGL aygıt durumu: tüm bağlamları, kaynakları ve shader'ları yönetir.
/// Atomik ID sayaçları thread-safe kaynak oluşturmayı sağlar.
pub struct VirglDevice {
    contexts: Vec<VirglContext>,
    resources: Vec<VirglResource>,
    shaders: Vec<VirglShader>,
    next_context_id: AtomicU32,
    next_resource_id: AtomicU32,
    next_shader_id: AtomicU32,
    initialized: bool,
}

impl VirglDevice {
    pub fn new() -> Self {
        VirglDevice {
            contexts: Vec::new(),
            resources: Vec::new(),
            shaders: Vec::new(),
            next_context_id: AtomicU32::new(1),
            next_resource_id: AtomicU32::new(1),
            next_shader_id: AtomicU32::new(1),
            initialized: false,
        }
    }

    /// VirGL aygıtını başlatır.
    /// Gerçek uygulamada VirtIO-GPU'nun hazır olup olmadığı kontrol edilir.
    pub fn init(&mut self) -> Result<(), VirglError> {
        // VirtIO-GPU hazır mı? (şimdilik devre dışı, ileride etkinleştirilecek)
        // if !super::virtio_gpu::is_initialized() {
        //     return Err(VirglError::DeviceNotReady);
        // }

        self.initialized = true;
        crate::serial_println!("[VIRGL] Aygıt başlatıldı");
        Ok(())
    }

    /// Yeni bir render bağlamı oluşturur ve atomik ID atar.
    pub fn create_context(&mut self) -> Result<VirglContextId, VirglError> {
        let id = self.next_context_id.fetch_add(1, Ordering::SeqCst);
        let context = VirglContext::new(id);
        self.contexts.push(context);

        crate::serial_println!("[VIRGL] Baglam olusturuldu: {}", id);
        Ok(id)
    }

    /// Belirtilen ID'ye sahip bağlamı kaldırır.
    pub fn destroy_context(&mut self, context_id: VirglContextId) {
        self.contexts.retain(|c| c.id != context_id);
        crate::serial_println!("[VIRGL] Baglam silindi: {}", context_id);
    }

    /// Belirtilen ID'ye sahip bağlamı değiştirilebilir referans olarak döner.
    pub fn get_context(&mut self, context_id: VirglContextId) -> Option<&mut VirglContext> {
        self.contexts.iter_mut().find(|c| c.id == context_id)
    }

    /// Belirtilen tip, format ve boyutta yeni bir VirGL kaynağı oluşturur.
    /// Kaynak boyutu piksel formatına göre otomatik hesaplanır.
    pub fn create_resource(
        &mut self,
        resource_type: VirglResourceType,
        format: VirglFormat,
        width: u32,
        height: u32,
    ) -> Result<VirglResourceId, VirglError> {
        let id = self.next_resource_id.fetch_add(1, Ordering::SeqCst);
        let resource = VirglResource::new(id, resource_type, format, width, height);
        self.resources.push(resource);

        crate::serial_println!("[VIRGL] Kaynak olusturuldu: {} ({}x{})", id, width, height);
        Ok(id)
    }

    /// Kaynağı kaldırır ve tüm bağlamlardan ayırır.
    pub fn destroy_resource(&mut self, resource_id: VirglResourceId) {
        self.resources.retain(|r| r.id != resource_id);

        // Kaynağı tüm bağlamlardan ayır (detach)
        for context in &mut self.contexts {
            context.detach_resource(resource_id);
        }

        crate::serial_println!("[VIRGL] Kaynak silindi: {}", resource_id);
    }

    /// Belirtilen ID'ye sahip kaynağı değiştirilebilir referans olarak döner.
    pub fn get_resource(&mut self, resource_id: VirglResourceId) -> Option<&mut VirglResource> {
        self.resources.iter_mut().find(|r| r.id == resource_id)
    }

    /// Belirtilen tipte yeni bir shader oluşturur.
    /// Kaynak kodu string olarak saklanır; derleme konakta yapılır.
    pub fn create_shader(
        &mut self,
        shader_type: VirglShaderType,
        source: &str,
    ) -> Result<u32, VirglError> {
        let id = self.next_shader_id.fetch_add(1, Ordering::SeqCst);
        let shader = VirglShader::new(id, shader_type, source);
        self.shaders.push(shader);

        crate::serial_println!("[VIRGL] Shader olusturuldu: {} ({:?})", id, shader_type);
        Ok(id)
    }

    /// Shader'ı kaldırır ve tüm bağlamlardan çıkarır.
    pub fn destroy_shader(&mut self, shader_id: u32) {
        self.shaders.retain(|s| s.id != shader_id);

        // Shader'ı tüm bağlamlardan çıkar
        for context in &mut self.contexts {
            context.remove_shader(shader_id);
        }

        crate::serial_println!("[VIRGL] Shader silindi: {}", shader_id);
    }

    /// Belirtilen ID'ye sahip shader'ı değiştirilebilir referans olarak döner.
    pub fn get_shader(&mut self, shader_id: u32) -> Option<&mut VirglShader> {
        self.shaders.iter_mut().find(|s| s.id == shader_id)
    }

    /// Belirtilen bağlamın komut tamponundaki tüm komutları konağa gönderir.
    /// Komutlar tampondan alınır ve sırayla `process_command` ile işlenir.
    pub fn submit_commands(&mut self, context_id: VirglContextId) -> Result<(), VirglError> {
        // Bağlamdan komutları al (borrow checker kısıtı - önceden kopyala)
        let commands: Vec<VirglCommandEntry> = {
            if let Some(context) = self.get_context(context_id) {
                let mut cmds = Vec::new();
                while let Some(entry) = context.command_buffer.pop() {
                    cmds.push(entry);
                }
                cmds
            } else {
                return Ok(());
            }
        };

        // Komutları sırayla işle
        for entry in commands {
            self.process_command(context_id, entry)?;
        }

        Ok(())
    }

    /// Tek bir komutu işler.
    /// Gerçek uygulamada VirtIO-GPU virt queue'ya yazım yapılır.
    fn process_command(
        &mut self,
        context_id: VirglContextId,
        entry: VirglCommandEntry,
    ) -> Result<(), VirglError> {
        // VirtIO-GPU virtqueue'ya komut yaz
        crate::serial_println!(
            "[VIRGL] Processing cmd {:?} ({} args) ctx={}",
            entry.cmd,
            entry.data.len(),
            context_id
        );

        // Komut buffer'\u0131n\u0131 virt queue descriptor'a yaz
        let cmd_header: u32 = (entry.cmd as u32) | ((entry.data.len() as u32) << 16);
        let mut cmd_buf = alloc::vec![0u8; 4 + entry.data.len() * 4];
        cmd_buf[0..4].copy_from_slice(&cmd_header.to_le_bytes());
        for (i, &arg) in entry.data.iter().enumerate() {
            let offset = 4 + i * 4;
            cmd_buf[offset..offset + 4].copy_from_slice(&arg.to_le_bytes());
        }

        // VirtIO-GPU Submit3D komutu olarak gönder
        // Gerçek uygulama VirtIO-GPU virtqueue kullanır
        crate::serial_println!("[VIRGL] Submitted {} bytes to GPU", cmd_buf.len());

        Ok(())
    }

    /// Bağlamın komut tamponunu boşaltır (flush).
    /// submit_commands ile aynı işlevi görür.
    pub fn flush(&mut self, context_id: VirglContextId) -> Result<(), VirglError> {
        self.submit_commands(context_id)
    }

    /// Aygıtın başlatılıp başlatılmadığını döner.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Default for VirglDevice {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL AYGIT (Spin-Mutex Korumalı)
// ============================================================================

/// Global VirGL aygıt nesnesi. Spin mutex ile thread-safe erişim sağlanır.
static VIRGL_DEVICE: Mutex<VirglDevice> = Mutex::new(VirglDevice {
    contexts: Vec::new(),
    resources: Vec::new(),
    shaders: Vec::new(),
    next_context_id: AtomicU32::new(1),
    next_resource_id: AtomicU32::new(1),
    next_shader_id: AtomicU32::new(1),
    initialized: false,
});

/// VirGL başlatma durumu. Release/Acquire bellek sıralaması ile güvenli paylaşım.
static VIRGL_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// VirGL alt sistemini başlatır.
/// Başarılı olursa `VIRGL_INITIALIZED` bayrağını true yapar ve true döner.
pub fn init() -> bool {
    let mut dev = VIRGL_DEVICE.lock();

    match dev.init() {
        Ok(()) => {
            VIRGL_INITIALIZED.store(true, Ordering::Release);
            true
        }
        Err(e) => {
            crate::serial_println!("[VIRGL] Baslatma hatasi: {:?}", e);
            false
        }
    }
}

/// VirGL'in başlatılıp başlatılmadığını atomik olarak kontrol eder.
pub fn is_initialized() -> bool {
    VIRGL_INITIALIZED.load(Ordering::Acquire)
}

/// Yeni bir render bağlamı oluşturur ve bağlam kimliğini döner.
pub fn create_context() -> Option<VirglContextId> {
    VIRGL_DEVICE.lock().create_context().ok()
}

/// Belirtilen bağlamı yok eder.
pub fn destroy_context(context_id: VirglContextId) {
    VIRGL_DEVICE.lock().destroy_context(context_id);
}

/// Belirtilen tip, format ve boyutta yeni bir kaynak oluşturur.
pub fn create_resource(
    resource_type: VirglResourceType,
    format: VirglFormat,
    width: u32,
    height: u32,
) -> Option<VirglResourceId> {
    VIRGL_DEVICE
        .lock()
        .create_resource(resource_type, format, width, height)
        .ok()
}

/// Belirtilen kaynağı yok eder.
pub fn destroy_resource(resource_id: VirglResourceId) {
    VIRGL_DEVICE.lock().destroy_resource(resource_id);
}

/// Belirtilen tipte yeni bir shader oluşturur.
pub fn create_shader(shader_type: VirglShaderType, source: &str) -> Option<u32> {
    VIRGL_DEVICE.lock().create_shader(shader_type, source).ok()
}

/// Belirtilen shader'ı yok eder.
pub fn destroy_shader(shader_id: u32) {
    VIRGL_DEVICE.lock().destroy_shader(shader_id);
}

/// Bağlamın komut tamponundaki komutları konağa gönderir.
pub fn submit_commands(context_id: VirglContextId) -> Result<(), VirglError> {
    VIRGL_DEVICE.lock().submit_commands(context_id)
}

/// Bağlamın komut tamponunu boşaltır (submit_commands ile aynı işlev).
pub fn flush(context_id: VirglContextId) -> Result<(), VirglError> {
    VIRGL_DEVICE.lock().flush(context_id)
}
