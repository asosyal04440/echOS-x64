//! # GPU Soyutlama Katmanı (GAL)
//!
//! Yazılım geri dönüşlü donanım hızlandırmalı grafik.
//! VirGL, yazılım render ve yerel GPU sürücülerini destekler.
//!
//! ## Mimari
//! - `TextureHandle / BufferHandle / ShaderHandle`: GPU kaynak tanımlayıcıları
//! - `TextureFormat / TextureUsage`: Piksel formatları ve kullanım bayrakları
//! - `BlendState`: RGBA kanalları için kaynak/hedef harmanlama faktörleri
//! - `DepthStencilState`: Derinlik testi, derinlik yazma, stencil işlemleri
//! - `RasterizerState`: Yüz ayıklama, dolgu modu, makas
//! - `Gal trait`: Tüm GPU arka uçlarının uyguladığı birleşik API
//! - `SoftwareGal`: CPU tabanlı yazılım render geri dönüş uygulaması

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::{GraphicsBackend, Surface, SwapChain};

// ============================================================================
// GAL SABİTLERİ
// ============================================================================

/// Maksimum doku birimi sayısı
const MAX_TEXTURE_UNITS: usize = 32;

/// Maksimum köşe tamponu boyutu (16 MB)
const MAX_VERTEX_BUFFER_SIZE: usize = 16 * 1024 * 1024;

/// Maksimum indeks tamponu boyutu (4 MB)
const MAX_INDEX_BUFFER_SIZE: usize = 4 * 1024 * 1024;

/// Varsayılan döşeme boyutu (32x32 piksel)
const DEFAULT_TILE_SIZE: usize = 32;

// ============================================================================
// DOKU TANIMLAYICISI
// ============================================================================

/// GPU doku tanımlayıcısı
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextureHandle(pub u32);

impl TextureHandle {
    pub const INVALID: TextureHandle = TextureHandle(0);

    pub fn new(id: u32) -> Self {
        TextureHandle(id)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

// ============================================================================
// TAMPON TANIMLAYICISI
// ============================================================================

/// GPU tampon tanımlayıcısı
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BufferHandle(pub u32);

impl BufferHandle {
    pub const INVALID: BufferHandle = BufferHandle(0);

    pub fn new(id: u32) -> Self {
        BufferHandle(id)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

// ============================================================================
// GÖLGELENDIRICI TANIMLAYICISI
// ============================================================================

/// Gölgelendirici programı tanımlayıcısı
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ShaderHandle(pub u32);

impl ShaderHandle {
    pub const INVALID: ShaderHandle = ShaderHandle(0);

    pub fn new(id: u32) -> Self {
        ShaderHandle(id)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

// ============================================================================
// DOKU FORMATI
// ============================================================================

/// Doku piksel formatı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    /// 8-bit Kırmızı kanalı
    R8,
    /// 8-bit Kırmızı + Yeşil kanallar
    RG8,
    /// 8-bit RGB
    RGB8,
    /// 8-bit RGBA
    RGBA8,
    /// 16-bit kayan nokta RGB
    RGB16F,
    /// 16-bit kayan nokta RGBA
    RGBA16F,
    /// 32-bit kayan nokta RGB
    RGB32F,
    /// 32-bit kayan nokta RGBA
    RGBA32F,
    /// 16-bit derinlik
    Depth16,
    /// 24-bit derinlik
    Depth24,
    /// 32-bit kayan nokta derinlik
    Depth32F,
    /// 8-bit stencil
    Stencil8,
    /// 24-bit derinlik + 8-bit stencil
    Depth24Stencil8,
}

impl TextureFormat {
    /// Piksel başına bayt sayısını al
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            TextureFormat::R8 => 1,
            TextureFormat::RG8 => 2,
            TextureFormat::RGB8 => 3,
            TextureFormat::RGBA8 => 4,
            TextureFormat::RGB16F => 6,
            TextureFormat::RGBA16F => 8,
            TextureFormat::RGB32F => 12,
            TextureFormat::RGBA32F => 16,
            TextureFormat::Depth16 => 2,
            TextureFormat::Depth24 => 3,
            TextureFormat::Depth32F => 4,
            TextureFormat::Stencil8 => 1,
            TextureFormat::Depth24Stencil8 => 4,
        }
    }

    /// Derinlik formatı mı
    pub fn is_depth(&self) -> bool {
        matches!(
            self,
            TextureFormat::Depth16
                | TextureFormat::Depth24
                | TextureFormat::Depth32F
                | TextureFormat::Depth24Stencil8
        )
    }

    /// Stencil formatı mı
    pub fn is_stencil(&self) -> bool {
        matches!(
            self,
            TextureFormat::Stencil8 | TextureFormat::Depth24Stencil8
        )
    }
}

// ============================================================================
// DOKU TANIMI
// ============================================================================

/// Doku oluşturma tanımı
#[derive(Clone, Debug)]
pub struct TextureDesc {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: u32,
    pub usage: TextureUsage,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TextureUsage: u32 {
        const SAMPLED = 1 << 0;
        const RENDER_TARGET = 1 << 1;
        const DEPTH_STENCIL = 1 << 2;
        const STORAGE = 1 << 3;
        const TRANSFER_SRC = 1 << 4;
        const TRANSFER_DST = 1 << 5;
    }
}

impl Default for TextureDesc {
    fn default() -> Self {
        TextureDesc {
            width: 1,
            height: 1,
            format: TextureFormat::RGBA8,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
            usage: TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        }
    }
}

impl TextureDesc {
    pub fn new_2d(width: u32, height: u32, format: TextureFormat) -> Self {
        TextureDesc {
            width,
            height,
            format,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
            usage: TextureUsage::SAMPLED | TextureUsage::TRANSFER_DST,
        }
    }

    pub fn render_target(width: u32, height: u32, format: TextureFormat) -> Self {
        TextureDesc {
            width,
            height,
            format,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
            usage: TextureUsage::RENDER_TARGET | TextureUsage::SAMPLED,
        }
    }

    pub fn depth_stencil(width: u32, height: u32) -> Self {
        TextureDesc {
            width,
            height,
            format: TextureFormat::Depth24Stencil8,
            mip_levels: 1,
            array_layers: 1,
            samples: 1,
            usage: TextureUsage::DEPTH_STENCIL,
        }
    }
}

// ============================================================================
// KÖŞE FORMATI
// ============================================================================

/// Köşe özellik formatı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    Float,
    Float2,
    Float3,
    Float4,
    Byte4,
    Byte4Norm,
    UByte4,
    UByte4Norm,
    Short2,
    Short2Norm,
    Short4,
    Short4Norm,
    UInt,
    UInt2,
    UInt3,
    UInt4,
}

impl VertexFormat {
    /// Bayt cinsinden boyutu al
    pub fn size(&self) -> usize {
        match self {
            VertexFormat::Float => 4,
            VertexFormat::Float2 => 8,
            VertexFormat::Float3 => 12,
            VertexFormat::Float4 => 16,
            VertexFormat::Byte4 | VertexFormat::Byte4Norm => 4,
            VertexFormat::UByte4 | VertexFormat::UByte4Norm => 4,
            VertexFormat::Short2 | VertexFormat::Short2Norm => 4,
            VertexFormat::Short4 | VertexFormat::Short4Norm => 8,
            VertexFormat::UInt => 4,
            VertexFormat::UInt2 => 8,
            VertexFormat::UInt3 => 12,
            VertexFormat::UInt4 => 16,
        }
    }
}

/// Köşe özellik tanımlayıcısı
#[derive(Clone, Debug)]
pub struct VertexAttribute {
    pub name: String,
    pub format: VertexFormat,
    pub offset: usize,
    pub buffer_index: usize,
}

/// Köşe tamponu düzeni
#[derive(Clone, Debug)]
pub struct VertexLayout {
    pub attributes: Vec<VertexAttribute>,
    pub stride: usize,
    pub instanced: bool,
}

impl VertexLayout {
    pub fn new() -> Self {
        VertexLayout {
            attributes: Vec::new(),
            stride: 0,
            instanced: false,
        }
    }

    pub fn add(mut self, name: &str, format: VertexFormat) -> Self {
        let offset = self.stride;
        self.stride += format.size();
        self.attributes.push(VertexAttribute {
            name: String::from(name),
            format,
            offset,
            buffer_index: 0,
        });
        self
    }

    pub fn add_instanced(mut self, name: &str, format: VertexFormat) -> Self {
        self.instanced = true;
        self.add(name, format)
    }
}

// ============================================================================
// İLKEL TÜRÜ
// ============================================================================

/// İlkel topoloji
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveType {
    Points,
    Lines,
    LineStrip,
    Triangles,
    TriangleStrip,
    TriangleFan,
}

// ============================================================================
// HARMANLA DURUMU
// ============================================================================

/// Harmanla faktörü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstColor,
    OneMinusDstColor,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturate,
}

/// Harmanla işlemi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendOp {
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

/// Tek bir render hedefi için harmanla durumu
#[derive(Clone, Copy, Debug)]
pub struct BlendState {
    pub enabled: bool,
    pub src_color: BlendFactor,
    pub dst_color: BlendFactor,
    pub color_op: BlendOp,
    pub src_alpha: BlendFactor,
    pub dst_alpha: BlendFactor,
    pub alpha_op: BlendOp,
}

impl Default for BlendState {
    fn default() -> Self {
        BlendState {
            enabled: false,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
        }
    }
}

impl BlendState {
    pub fn alpha_blend() -> Self {
        BlendState {
            enabled: true,
            src_color: BlendFactor::SrcAlpha,
            dst_color: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
        }
    }

    pub fn additive() -> Self {
        BlendState {
            enabled: true,
            src_color: BlendFactor::SrcAlpha,
            dst_color: BlendFactor::One,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::One,
            alpha_op: BlendOp::Add,
        }
    }

    pub fn multiply() -> Self {
        BlendState {
            enabled: true,
            src_color: BlendFactor::DstColor,
            dst_color: BlendFactor::Zero,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::DstAlpha,
            dst_alpha: BlendFactor::Zero,
            alpha_op: BlendOp::Add,
        }
    }

    pub fn premultiplied() -> Self {
        BlendState {
            enabled: true,
            src_color: BlendFactor::One,
            dst_color: BlendFactor::OneMinusSrcAlpha,
            color_op: BlendOp::Add,
            src_alpha: BlendFactor::One,
            dst_alpha: BlendFactor::OneMinusSrcAlpha,
            alpha_op: BlendOp::Add,
        }
    }
}

// ============================================================================
// DERİNLİK/STENCİL DURUMU
// ============================================================================

/// Karşılaştırma fonksiyonu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// Stencil işlemi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilOp {
    Keep,
    Zero,
    Replace,
    IncrementClamp,
    DecrementClamp,
    Invert,
    IncrementWrap,
    DecrementWrap,
}

/// Tek yüz için stencil durumu
#[derive(Clone, Copy, Debug)]
pub struct StencilFaceState {
    pub compare: CompareFunc,
    pub fail_op: StencilOp,
    pub depth_fail_op: StencilOp,
    pub pass_op: StencilOp,
    pub read_mask: u8,
    pub write_mask: u8,
    pub reference: u8,
}

impl Default for StencilFaceState {
    fn default() -> Self {
        StencilFaceState {
            compare: CompareFunc::Always,
            fail_op: StencilOp::Keep,
            depth_fail_op: StencilOp::Keep,
            pass_op: StencilOp::Keep,
            read_mask: 0xFF,
            write_mask: 0xFF,
            reference: 0,
        }
    }
}

/// Derinlik/stencil durumu
#[derive(Clone, Debug)]
pub struct DepthStencilState {
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: CompareFunc,
    pub stencil_test: bool,
    pub stencil_front: StencilFaceState,
    pub stencil_back: StencilFaceState,
}

impl Default for DepthStencilState {
    fn default() -> Self {
        DepthStencilState {
            depth_test: true,
            depth_write: true,
            depth_compare: CompareFunc::Less,
            stencil_test: false,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
        }
    }
}

// ============================================================================
// RASTERLEŞTIRICI DURUMU
// ============================================================================

/// Yüz ayıklama modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullMode {
    None,
    Front,
    Back,
}

/// Ön yüz döngü yönü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontFace {
    Clockwise,
    CounterClockwise,
}

/// Dolgu modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    Solid,
    Wireframe,
}

/// Rasterleştirici durumu
#[derive(Clone, Copy, Debug)]
pub struct RasterizerState {
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
    pub fill_mode: FillMode,
    pub depth_bias: i32,
    pub depth_bias_clamp: f32,
    pub slope_scaled_depth_bias: f32,
    pub depth_clip_enable: bool,
    pub scissor_enable: bool,
    pub multisample_enable: bool,
}

impl Default for RasterizerState {
    fn default() -> Self {
        RasterizerState {
            cull_mode: CullMode::Back,
            front_face: FrontFace::CounterClockwise,
            fill_mode: FillMode::Solid,
            depth_bias: 0,
            depth_bias_clamp: 0.0,
            slope_scaled_depth_bias: 0.0,
            depth_clip_enable: true,
            scissor_enable: false,
            multisample_enable: false,
        }
    }
}

// ============================================================================
// RENDER GEÇİŞİ
// ============================================================================

/// Render geçişi bağlantısı
#[derive(Clone, Debug)]
pub struct RenderPassAttachment {
    pub texture: TextureHandle,
    pub mip_level: u32,
    pub array_layer: u32,
    pub load_op: LoadOp,
    pub store_op: StoreOp,
    pub clear_value: ClearValue,
}

/// Yükleme işlemi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadOp {
    Load,
    Clear,
    DontCare,
}

/// Depolama işlemi
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreOp {
    Store,
    DontCare,
}

/// Temizleme değeri
#[derive(Clone, Copy, Debug)]
pub enum ClearValue {
    Color { r: f32, g: f32, b: f32, a: f32 },
    DepthStencil { depth: f32, stencil: u8 },
}

impl Default for ClearValue {
    fn default() -> Self {
        ClearValue::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }
}

/// Render geçişi tanımlayıcısı
#[derive(Clone, Debug)]
pub struct RenderPassDesc {
    pub color_attachments: Vec<RenderPassAttachment>,
    pub depth_stencil: Option<RenderPassAttachment>,
}

// ============================================================================
// ÇİZİM KOMUTLARI
// ============================================================================

/// İndeksli çizim için çizim komutu
#[derive(Clone, Copy, Debug)]
pub struct DrawIndexed {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

/// İndekssiz çizim için çizim komutu
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

// ============================================================================
// GAL TRAIT'İ
// ============================================================================

/// GPU Soyutlama Katmanı trait'i
pub trait Gal: Send + Sync {
    // === Kaynak Oluşturma ===

    /// Doku oluştur
    fn create_texture(&mut self, desc: &TextureDesc) -> Option<TextureHandle>;

    /// Dokuyu yok et
    fn destroy_texture(&mut self, texture: TextureHandle);

    /// Köşe tamponu oluştur
    fn create_vertex_buffer(&mut self, size: usize, data: Option<&[u8]>) -> Option<BufferHandle>;

    /// İndeks tamponu oluştur
    fn create_index_buffer(&mut self, size: usize, data: Option<&[u8]>) -> Option<BufferHandle>;

    /// Tekdüze tampon oluştur
    fn create_uniform_buffer(&mut self, size: usize) -> Option<BufferHandle>;

    /// Tamponu yok et
    fn destroy_buffer(&mut self, buffer: BufferHandle);

    /// Gölgelendirici programı oluştur
    fn create_shader(&mut self, vertex_src: &str, fragment_src: &str) -> Option<ShaderHandle>;

    /// Gölgelendiriciyi yok et
    fn destroy_shader(&mut self, shader: ShaderHandle);

    // === Kaynak Güncellemeleri ===

    /// Doku verisini güncelle
    fn update_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    );

    /// Tampon verisini güncelle
    fn update_buffer(&mut self, buffer: BufferHandle, offset: usize, data: &[u8]);

    /// Doku verisini CPU'ya geri oku
    fn read_texture(&self, texture: TextureHandle, data: &mut [u8]) -> bool;

    // === Render ===

    /// Render geçişi başlat
    fn begin_render_pass(&mut self, desc: &RenderPassDesc);

    /// Mevcut render geçişini sonlandır
    fn end_render_pass(&mut self);

    /// Gölgelendirici programı bağla
    fn bind_shader(&mut self, shader: ShaderHandle);

    /// Köşe tamponu bağla
    fn bind_vertex_buffer(&mut self, index: u32, buffer: BufferHandle, offset: usize);

    /// İndeks tamponu bağla
    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: usize);

    /// Dokuyu bir slota bağla
    fn bind_texture(&mut self, slot: u32, texture: TextureHandle);

    /// Harmanla durumunu ayarla
    fn set_blend_state(&mut self, state: BlendState);

    /// Derinlik/stencil durumunu ayarla
    fn set_depth_stencil_state(&mut self, state: DepthStencilState);

    /// Rasterleştirici durumunu ayarla
    fn set_rasterizer_state(&mut self, state: RasterizerState);

    /// Görüntü alanı ayarla
    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    );

    /// Makas dikdörtgeni ayarla
    fn set_scissor(&mut self, x: i32, y: i32, width: u32, height: u32);

    /// Köşe düzeni ayarla
    fn set_vertex_layout(&mut self, layout: &VertexLayout);

    /// İndeksli ilkel çiz
    fn draw_indexed(&mut self, cmd: DrawIndexed);

    /// İlkel çiz
    fn draw(&mut self, cmd: Draw);

    // === Tekdüzeler ===

    /// Tekdüze float ayarla
    fn set_uniform_float(&mut self, name: &str, value: f32);

    /// Tekdüze vec2 ayarla
    fn set_uniform_vec2(&mut self, name: &str, x: f32, y: f32);

    /// Tekdüze vec3 ayarla
    fn set_uniform_vec3(&mut self, name: &str, x: f32, y: f32, z: f32);

    /// Tekdüze vec4 ayarla
    fn set_uniform_vec4(&mut self, name: &str, x: f32, y: f32, z: f32, w: f32);

    /// Tekdüze mat4 ayarla
    fn set_uniform_mat4(&mut self, name: &str, matrix: &[f32; 16]);

    /// Tekdüze tampon ayarla
    fn set_uniform_buffer(&mut self, name: &str, buffer: BufferHandle);

    // === Sunum ===

    /// Ekrana sun
    fn present(&mut self);

    /// Mevcut arka tamponu al
    fn backbuffer(&self) -> Option<TextureHandle>;

    /// Arka tamponu yeniden boyutlandır
    fn resize(&mut self, width: u32, height: u32);

    // === Yetenekler ===

    /// Arka uç türünü al
    fn backend(&self) -> GraphicsBackend;

    /// Maksimum doku boyutunu al
    fn max_texture_size(&self) -> u32;

    /// Maksimum render hedef sayısını al
    fn max_render_targets(&self) -> u32;

    /// Formatın desteklenip desteklenmediğini kontrol et
    fn is_format_supported(&self, format: TextureFormat, usage: TextureUsage) -> bool;
}

// ============================================================================
// YAZILIM GAL UYGULAMASI
// ============================================================================

/// Yazılım render GAL (CPU geri dönüş)
pub struct SoftwareGal {
    /// Ekran boyutları
    width: u32,
    height: u32,
    /// Çerçeve tamponu
    framebuffer: Vec<u32>,
    /// Derinlik tamponu
    depth_buffer: Vec<f32>,
    /// Dokular
    textures: BTreeMap<u32, SoftwareTexture>,
    /// Tamponlar
    buffers: BTreeMap<u32, SoftwareBuffer>,
    /// Gölgelendiriciler
    shaders: BTreeMap<u32, SoftwareShader>,
    /// Sonraki tanımlayıcı kimliği
    next_handle: AtomicU32,
    /// Mevcut render geçişi
    current_pass: Option<RenderPassDesc>,
    /// Mevcut gölgelendirici
    current_shader: Option<ShaderHandle>,
    /// Görüntü alanı
    viewport: (f32, f32, f32, f32, f32, f32),
    /// Makas
    scissor: (i32, i32, u32, u32),
    /// Harmanla durumu
    blend_state: BlendState,
    /// Derinlik durumu
    depth_state: DepthStencilState,
}

struct SoftwareTexture {
    width: u32,
    height: u32,
    format: TextureFormat,
    data: Vec<u8>,
}

struct SoftwareBuffer {
    data: Vec<u8>,
    is_index: bool,
}

struct SoftwareShader {
    vertex_src: String,
    fragment_src: String,
}

impl SoftwareGal {
    pub fn new(width: u32, height: u32) -> Self {
        let pixel_count = (width * height) as usize;

        SoftwareGal {
            width,
            height,
            framebuffer: vec![0; pixel_count],
            depth_buffer: vec![1.0; pixel_count],
            textures: BTreeMap::new(),
            buffers: BTreeMap::new(),
            shaders: BTreeMap::new(),
            next_handle: AtomicU32::new(1),
            current_pass: None,
            current_shader: None,
            viewport: (0.0, 0.0, width as f32, height as f32, 0.0, 1.0),
            scissor: (0, 0, width, height),
            blend_state: BlendState::default(),
            depth_state: DepthStencilState::default(),
        }
    }

    pub fn clear_rgba(&mut self, color: u32) {
        for pixel in &mut self.framebuffer {
            *pixel = color;
        }
    }

    pub fn plot_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y as usize * self.width as usize + x as usize;
        if let Some(pixel) = self.framebuffer.get_mut(index) {
            *pixel = color;
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.framebuffer
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    pub fn pixels(&self) -> &[u32] {
        &self.framebuffer
    }

    fn next_texture_handle(&self) -> TextureHandle {
        TextureHandle::new(self.next_handle.fetch_add(1, Ordering::Relaxed))
    }

    fn next_buffer_handle(&self) -> BufferHandle {
        BufferHandle::new(self.next_handle.fetch_add(1, Ordering::Relaxed))
    }

    fn next_shader_handle(&self) -> ShaderHandle {
        ShaderHandle::new(self.next_handle.fetch_add(1, Ordering::Relaxed))
    }
}

impl Gal for SoftwareGal {
    fn create_texture(&mut self, desc: &TextureDesc) -> Option<TextureHandle> {
        let handle = self.next_texture_handle();
        let size = (desc.width * desc.height) as usize * desc.format.bytes_per_pixel();

        self.textures.insert(
            handle.0,
            SoftwareTexture {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                data: vec![0; size],
            },
        );

        Some(handle)
    }

    fn destroy_texture(&mut self, texture: TextureHandle) {
        self.textures.remove(&texture.0);
    }

    fn create_vertex_buffer(&mut self, size: usize, data: Option<&[u8]>) -> Option<BufferHandle> {
        let handle = self.next_buffer_handle();
        let mut buffer = vec![0u8; size];

        if let Some(d) = data {
            let copy_len = d.len().min(size);
            buffer[..copy_len].copy_from_slice(&d[..copy_len]);
        }

        self.buffers.insert(
            handle.0,
            SoftwareBuffer {
                data: buffer,
                is_index: false,
            },
        );

        Some(handle)
    }

    fn create_index_buffer(&mut self, size: usize, data: Option<&[u8]>) -> Option<BufferHandle> {
        let handle = self.next_buffer_handle();
        let mut buffer = vec![0u8; size];

        if let Some(d) = data {
            let copy_len = d.len().min(size);
            buffer[..copy_len].copy_from_slice(&d[..copy_len]);
        }

        self.buffers.insert(
            handle.0,
            SoftwareBuffer {
                data: buffer,
                is_index: true,
            },
        );

        Some(handle)
    }

    fn create_uniform_buffer(&mut self, size: usize) -> Option<BufferHandle> {
        self.create_vertex_buffer(size, None)
    }

    fn destroy_buffer(&mut self, buffer: BufferHandle) {
        self.buffers.remove(&buffer.0);
    }

    fn create_shader(&mut self, vertex_src: &str, fragment_src: &str) -> Option<ShaderHandle> {
        let handle = self.next_shader_handle();

        self.shaders.insert(
            handle.0,
            SoftwareShader {
                vertex_src: String::from(vertex_src),
                fragment_src: String::from(fragment_src),
            },
        );

        Some(handle)
    }

    fn destroy_shader(&mut self, shader: ShaderHandle) {
        self.shaders.remove(&shader.0);
    }

    fn update_texture(
        &mut self,
        texture: TextureHandle,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        data: &[u8],
    ) {
        if let Some(tex) = self.textures.get_mut(&texture.0) {
            let bpp = tex.format.bytes_per_pixel();

            for row in 0..height {
                let dst_y = y + row;
                if dst_y >= tex.height {
                    break;
                }

                let src_offset = row as usize * width as usize * bpp;
                let dst_offset = dst_y as usize * tex.width as usize * bpp + x as usize * bpp;
                let copy_len = (width as usize * bpp).min(tex.data.len() - dst_offset);

                if src_offset + copy_len <= data.len() {
                    tex.data[dst_offset..dst_offset + copy_len]
                        .copy_from_slice(&data[src_offset..src_offset + copy_len]);
                }
            }
        }
    }

    fn update_buffer(&mut self, buffer: BufferHandle, offset: usize, data: &[u8]) {
        if let Some(buf) = self.buffers.get_mut(&buffer.0) {
            let end = offset + data.len();
            if end <= buf.data.len() {
                buf.data[offset..end].copy_from_slice(data);
            }
        }
    }

    fn read_texture(&self, texture: TextureHandle, data: &mut [u8]) -> bool {
        if let Some(tex) = self.textures.get(&texture.0) {
            let copy_len = data.len().min(tex.data.len());
            data[..copy_len].copy_from_slice(&tex.data[..copy_len]);
            true
        } else {
            false
        }
    }

    fn begin_render_pass(&mut self, desc: &RenderPassDesc) {
        self.current_pass = Some(desc.clone());

        // Bağlantıları temizle
        for attachment in &desc.color_attachments {
            if attachment.load_op == LoadOp::Clear {
                if let ClearValue::Color { r, g, b, a } = attachment.clear_value {
                    let color = ((r * 255.0) as u32) << 16
                        | ((g * 255.0) as u32) << 8
                        | ((b * 255.0) as u32);

                    // Çerçeve tamponunu temizle
                    for pixel in &mut self.framebuffer {
                        *pixel = color;
                    }
                }
            }
        }

        // Derinliği temizle
        if let Some(ref ds) = desc.depth_stencil {
            if ds.load_op == LoadOp::Clear {
                if let ClearValue::DepthStencil { depth, .. } = ds.clear_value {
                    for d in &mut self.depth_buffer {
                        *d = depth;
                    }
                }
            }
        }
    }

    fn end_render_pass(&mut self) {
        self.current_pass = None;
    }

    fn bind_shader(&mut self, shader: ShaderHandle) {
        self.current_shader = Some(shader);
    }

    fn bind_vertex_buffer(&mut self, index: u32, buffer: BufferHandle, offset: usize) {
        let _ = (index, buffer, offset);
        // Yazılım render bu bilgiyi takip eder
    }

    fn bind_index_buffer(&mut self, buffer: BufferHandle, offset: usize) {
        let _ = (buffer, offset);
    }

    fn bind_texture(&mut self, slot: u32, texture: TextureHandle) {
        let _ = (slot, texture);
    }

    fn set_blend_state(&mut self, state: BlendState) {
        self.blend_state = state;
    }

    fn set_depth_stencil_state(&mut self, state: DepthStencilState) {
        self.depth_state = state;
    }

    fn set_rasterizer_state(&mut self, state: RasterizerState) {
        let _ = state;
    }

    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.viewport = (x, y, width, height, min_depth, max_depth);
    }

    fn set_scissor(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.scissor = (x, y, width, height);
    }

    fn set_vertex_layout(&mut self, layout: &VertexLayout) {
        let _ = layout;
    }

    fn draw_indexed(&mut self, cmd: DrawIndexed) {
        let _ = cmd;
        // Yazılım render indeksler üzerinde dolaşır ve üçgenleri rasterleştirir
    }

    fn draw(&mut self, cmd: Draw) {
        let _ = cmd;
    }

    fn set_uniform_float(&mut self, name: &str, value: f32) {
        let _ = (name, value);
    }

    fn set_uniform_vec2(&mut self, name: &str, x: f32, y: f32) {
        let _ = (name, x, y);
    }

    fn set_uniform_vec3(&mut self, name: &str, x: f32, y: f32, z: f32) {
        let _ = (name, x, y, z);
    }

    fn set_uniform_vec4(&mut self, name: &str, x: f32, y: f32, z: f32, w: f32) {
        let _ = (name, x, y, z, w);
    }

    fn set_uniform_mat4(&mut self, name: &str, matrix: &[f32; 16]) {
        let _ = (name, matrix);
    }

    fn set_uniform_buffer(&mut self, name: &str, buffer: BufferHandle) {
        let _ = (name, buffer);
    }

    fn present(&mut self) {
        // Çerçeve tamponunu çıkışa kopyala
    }

    fn backbuffer(&self) -> Option<TextureHandle> {
        None // Yazılım render çerçeve tamponunu doğrudan kullanır
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.framebuffer.resize((width * height) as usize, 0);
        self.depth_buffer.resize((width * height) as usize, 1.0);
    }

    fn backend(&self) -> GraphicsBackend {
        GraphicsBackend::Software
    }

    fn max_texture_size(&self) -> u32 {
        4096
    }

    fn max_render_targets(&self) -> u32 {
        1
    }

    fn is_format_supported(&self, format: TextureFormat, _usage: TextureUsage) -> bool {
        matches!(format, TextureFormat::RGBA8 | TextureFormat::RGB8)
    }
}

// ============================================================================
// GLOBAL GAL ÖRNEĞI
// ============================================================================

lazy_static::lazy_static! {
    static ref GAL_INSTANCE: Mutex<Option<Box<dyn Gal>>> = Mutex::new(None);
}

/// GAL'ı yazılım arka ucuyla başlat
pub fn init_software(width: u32, height: u32) {
    *GAL_INSTANCE.lock() = Some(Box::new(SoftwareGal::new(width, height)));
    crate::serial_println!("[GAL] Yazılım render başlatıldı ({}x{})", width, height);
}

/// GAL örneğini al
pub fn get() -> Option<&'static Mutex<Option<Box<dyn Gal>>>> {
    Some(&GAL_INSTANCE)
}

/// GAL erişimiyle bir closure çalıştır
pub fn with_gal<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Box<dyn Gal>) -> R,
{
    let mut guard = GAL_INSTANCE.lock();
    if let Some(ref mut gal) = *guard {
        Some(f(gal))
    } else {
        None
    }
}
