//! # GPU 3D API — Donanım Hızlandırmalı 3D Grafik API'si
//!
//! Vulkan benzeri grafik API'si — donanım hızlandırmalı 3D render için.
//!
//! ## Vulkan Mimarisi Hakkında
//! Bu modül, Vulkan API tasarım felsefesini takip eder:
//! - **Handle tabanlı kaynak yönetimi**: Her kaynak benzersiz bir u64 handle ile tanımlanır
//! - **Explicit senkronizasyon**: Bariyer ve fence'lar programcı tarafından yönetilir
//! - **Render Pass**: Render hedefleri ve alt geçişler açıkça tanımlanır
//! - **SPIR-V shader**: Shader kodu doğrudan SPIR-V bayt kodu olarak verilir

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

// ============================================================================
// GPU SABİTLERİ
// ============================================================================

/// Desteklenen maksimum doku (texture) boyutu — piksel cinsinden (4096x4096)
const MAX_TEXTURE_SIZE: u32 = 4096;

/// Aynı anda bağlanabilecek maksimum köşe noktası (vertex) tampon sayısı
const MAX_VERTEX_BUFFERS: usize = 16;

/// Maksimum tanımlayıcı küme (descriptor set) sayısı
const MAX_DESCRIPTOR_SETS: usize = 8;

/// Push constant için maksimum veri boyutu (bayt).
/// Push constant, pipeline'a çok hızlı küçük veri geçirmek için kullanılır.
const MAX_PUSH_CONSTANT_SIZE: usize = 128;

/// Maksimum render hedefi sayısı (çoklu render target — MRT)
const MAX_RENDER_TARGETS: usize = 8;

/// Maksimum viewport sayısı
const MAX_VIEWPORTS: usize = 16;

/// Maksimum makas (scissor) sayısı
const MAX_SCISSORS: usize = 16;

// ============================================================================
// GPU HATALARI
// ============================================================================

/// GPU işlem hatası türleri.
/// Her varyant, farklı bir hata koşulunu temsil eder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuError {
    /// GPU cihazı kayboldu (genellikle donanım arızası)
    DeviceLost,
    /// GPU belleği yetersiz
    OutOfMemory,
    /// Geçersiz kaynak handle'ı
    InvalidHandle,
    /// Desteklenmeyen format
    InvalidFormat,
    /// Geçersiz kullanım bayrağı
    InvalidUsage,
    /// Shader derleme başarısız
    ShaderCompileFailed,
    /// Pipeline oluşturma başarısız
    PipelineCreateFailed,
    /// Render pass eksik yapılandırma
    RenderPassIncomplete,
    /// Komut tamponu dolu
    CommandBufferFull,
    /// Zaman aşımı
    Timeout,
    /// İşlem desteklenmiyor
    NotSupported,
}

// ============================================================================
// GPU FORMATLARI
// ============================================================================

/// Piksel ve doku formatları.
/// Unorm = Normalleştirilmemiş işaretsiz tam sayı (0-255 → 0.0-1.0 aralığına normalize edilir).
/// Snorm = Normalleştirilmiş işaretli tam sayı (-127..127 → -1.0..1.0).
/// Sfloat = İşaretli kayan noktalı sayı (IEEE 754 formatı).
/// Srgb = sRGB renk uzayı (gama düzeltmeli).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Undefined,

    // Renk formatları
    R8Unorm,
    R8Snorm,
    R8Uint,
    R8Sint,
    R8G8Unorm,
    R8G8Snorm,
    R8G8Uint,
    R8G8Sint,
    R8G8B8Unorm,
    R8G8B8Snorm,
    R8G8B8A8Unorm,
    R8G8B8A8Snorm,
    B8G8R8A8Unorm,
    B8G8R8A8Srgb,
    R8G8B8A8Srgb,

    // Derinlik formatları (Depth) — z-buffer ve stencil için kullanılır
    D16Unorm,
    D24Unorm,
    D32Sfloat,
    D16UnormS8Uint,
    D24UnormS8Uint,
    D32SfloatS8Uint,

    // Sıkıştırılmış formatlar (BC = Block Compression) — sabit oranlı GPU doku sıkıştırması
    BC1RgbUnorm,
    BC1RgbaUnorm,
    BC2Unorm,
    BC3Unorm,
    BC4Unorm,
    BC5Unorm,
    BC6HUfloat,
    BC6HSfloat,
    BC7Unorm,

    // Kayan noktalı (floating point) formatlar — HDR render için
    R16Sfloat,
    R16G16Sfloat,
    R16G16B16Sfloat,
    R16G16B16A16Sfloat,
    R32Sfloat,
    R32G32Sfloat,
    R32G32B32Sfloat,
    R32G32B32A32Sfloat,
}

impl Format {
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            Format::Undefined => 0,
            Format::R8Unorm | Format::R8Snorm | Format::R8Uint | Format::R8Sint => 1,
            Format::R8G8Unorm | Format::R8G8Snorm | Format::R8G8Uint | Format::R8G8Sint => 2,
            Format::R8G8B8Unorm | Format::R8G8B8Snorm => 3,
            Format::R8G8B8A8Unorm
            | Format::R8G8B8A8Snorm
            | Format::B8G8R8A8Unorm
            | Format::B8G8R8A8Srgb
            | Format::R8G8B8A8Srgb => 4,
            Format::D16Unorm => 2,
            Format::D24Unorm => 3,
            Format::D32Sfloat => 4,
            Format::D16UnormS8Uint => 3,
            Format::D24UnormS8Uint => 4,
            Format::D32SfloatS8Uint => 5,
            Format::R16Sfloat => 2,
            Format::R16G16Sfloat => 4,
            Format::R16G16B16Sfloat => 6,
            Format::R16G16B16A16Sfloat => 8,
            Format::R32Sfloat => 4,
            Format::R32G32Sfloat => 8,
            Format::R32G32B32Sfloat => 12,
            Format::R32G32B32A32Sfloat => 16,
            _ => 0, // Sıkıştırılmış formatlar değişkendir
        }
    }

    pub fn is_depth(&self) -> bool {
        matches!(
            self,
            Format::D16Unorm
                | Format::D24Unorm
                | Format::D32Sfloat
                | Format::D16UnormS8Uint
                | Format::D24UnormS8Uint
                | Format::D32SfloatS8Uint
        )
    }

    pub fn is_stencil(&self) -> bool {
        matches!(
            self,
            Format::D16UnormS8Uint | Format::D24UnormS8Uint | Format::D32SfloatS8Uint
        )
    }

    pub fn is_color(&self) -> bool {
        !self.is_depth() && !self.is_stencil() && *self != Format::Undefined
    }
}

// ============================================================================
// GPU KAYNAKLARI — HANDLE TANIMLARI
// ============================================================================

/// Her kaynak türü için ayrı tip güvenlikli handle.
/// Wrap edilmiş u64, farklı handle türlerinin karıştırılmasını derleme zamanında önler.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageViewHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SamplerHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShaderHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PipelineHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderPassHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FramebufferHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommandBufferHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DescriptorSetHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FenceHandle(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SemaphoreHandle(pub u64);

// ============================================================================
// TAMPON (BUFFER)
// ============================================================================

/// Tampon kullanım bayrakları.
/// Her boolean, tamponu belirli bir amaçla bağlamaya izin verir.
/// Bu tasarım, GPU'nun optimizasyon yapmasını sağlar — sadece gerekli
/// erişim türleri etkinleştirilir.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferUsage {
    pub transfer_src: bool,
    pub transfer_dst: bool,
    pub uniform_texel: bool,
    pub storage_texel: bool,
    pub uniform: bool,
    pub storage: bool,
    pub index: bool,
    pub vertex: bool,
    pub indirect: bool,
}

impl BufferUsage {
    pub fn vertex() -> Self {
        BufferUsage {
            vertex: true,
            transfer_dst: true,
            ..Self::none()
        }
    }

    pub fn index() -> Self {
        BufferUsage {
            index: true,
            transfer_dst: true,
            ..Self::none()
        }
    }

    pub fn uniform() -> Self {
        BufferUsage {
            uniform: true,
            ..Self::none()
        }
    }

    pub fn storage() -> Self {
        BufferUsage {
            storage: true,
            ..Self::none()
        }
    }

    pub fn none() -> Self {
        BufferUsage {
            transfer_src: false,
            transfer_dst: false,
            uniform_texel: false,
            storage_texel: false,
            uniform: false,
            storage: false,
            index: false,
            vertex: false,
            indirect: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BufferDesc {
    pub size: u64,
    pub usage: BufferUsage,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Buffer {
    pub handle: BufferHandle,
    pub desc: BufferDesc,
    pub mapped: bool,
    pub data: Vec<u8>,
}

impl Buffer {
    pub fn new(handle: BufferHandle, desc: BufferDesc) -> Self {
        let data = vec![0u8; desc.size as usize];
        Buffer {
            handle,
            desc,
            mapped: false,
            data,
        }
    }

    pub fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GpuError> {
        let start = offset as usize;
        let end = start + data.len();
        if end > self.data.len() {
            return Err(GpuError::OutOfMemory);
        }
        self.data[start..end].copy_from_slice(data);
        Ok(())
    }

    pub fn read(&self, offset: u64, size: u64) -> Result<&[u8], GpuError> {
        let start = offset as usize;
        let end = start + size as usize;
        if end > self.data.len() {
            return Err(GpuError::OutOfMemory);
        }
        Ok(&self.data[start..end])
    }
}

// ============================================================================
// GÖRÜNTÜ (IMAGE / TEXTURE)
// ============================================================================

/// GPU görüntüsü (image) — 1D/2D/3D doku veya render hedefi olarak kullanılır.
/// Vulkan'da image ve framebuffer ayrımı vardır:
/// - Image: ham piksel verisi
/// - ImageView: image'ın belirli bir alt kümesine bakış (view)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageType {
    Dim1D,
    Dim2D,
    Dim3D,
    Cube,
    Dim1DArray,
    Dim2DArray,
    CubeArray,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageUsage {
    pub transfer_src: bool,
    pub transfer_dst: bool,
    pub sampled: bool,
    pub storage: bool,
    pub color_attachment: bool,
    pub depth_stencil_attachment: bool,
    pub input_attachment: bool,
}

impl ImageUsage {
    pub fn color_attachment() -> Self {
        ImageUsage {
            color_attachment: true,
            transfer_dst: true,
            ..Self::none()
        }
    }

    pub fn depth_stencil() -> Self {
        ImageUsage {
            depth_stencil_attachment: true,
            ..Self::none()
        }
    }

    pub fn sampled() -> Self {
        ImageUsage {
            sampled: true,
            transfer_dst: true,
            ..Self::none()
        }
    }

    pub fn storage() -> Self {
        ImageUsage {
            storage: true,
            ..Self::none()
        }
    }

    pub fn none() -> Self {
        ImageUsage {
            transfer_src: false,
            transfer_dst: false,
            sampled: false,
            storage: false,
            color_attachment: false,
            depth_stencil_attachment: false,
            input_attachment: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageDesc {
    pub image_type: ImageType,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub usage: ImageUsage,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Image {
    pub handle: ImageHandle,
    pub desc: ImageDesc,
    pub data: Vec<u8>,
}

impl Image {
    pub fn new(handle: ImageHandle, desc: ImageDesc) -> Self {
        let size = (desc.width * desc.height * desc.depth * desc.format.bytes_per_pixel()) as usize;
        let data = vec![0u8; size];
        Image { handle, desc, data }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), GpuError> {
        if data.len() > self.data.len() {
            return Err(GpuError::OutOfMemory);
        }
        self.data.copy_from_slice(data);
        Ok(())
    }
}

// ============================================================================
// GÖRÜNTÜ GÖRÜNÜMÜ (IMAGE VIEW)
// ============================================================================

/// ImageView, bir Image'ın belirli bir katman/mip seviyesini veya biçim
/// dönüşümünü temsil eder. Shader'da bağlı olan ImageView'dir, Image değil.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageViewType {
    Dim1D,
    Dim2D,
    Dim3D,
    Cube,
    Dim1DArray,
    Dim2DArray,
    CubeArray,
}

#[derive(Clone, Debug)]
pub struct ImageViewDesc {
    pub image: ImageHandle,
    pub view_type: ImageViewType,
    pub format: Format,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[derive(Clone, Debug)]
pub struct ImageView {
    pub handle: ImageViewHandle,
    pub desc: ImageViewDesc,
}

// ============================================================================
// ÖRNEKLEYICI (SAMPLER)
// ============================================================================

/// Doku örnekleyici — shader'ın dokuya nasıl erişeceğini tanımlar.
/// Filtreleme modu (yakın/doğrusal), MIP haritası, adres modu ve
/// anizotropik filtreleme gibi ayarları içerir.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipmapMode {
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressMode {
    Repeat,
    MirroredRepeat,
    ClampToEdge,
    ClampToBorder,
    MirrorClampToEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderColor {
    FloatTransparentBlack,
    IntTransparentBlack,
    FloatOpaqueBlack,
    IntOpaqueBlack,
    FloatOpaqueWhite,
    IntOpaqueWhite,
}

#[derive(Clone, Debug)]
pub struct SamplerDesc {
    pub mag_filter: Filter,
    pub min_filter: Filter,
    pub mipmap_mode: MipmapMode,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mip_lod_bias: f32,
    pub anisotropy_enable: bool,
    pub max_anisotropy: f32,
    pub compare_enable: bool,
    pub compare_op: CompareOp,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: BorderColor,
    pub unnormalized_coordinates: bool,
}

#[derive(Clone, Debug)]
pub struct Sampler {
    pub handle: SamplerHandle,
    pub desc: SamplerDesc,
}

// ============================================================================
// SHADER (GÖLGELENDIRICI)
// ============================================================================

/// Shader programı — GPU'da çalışan paralel işlemci kodu.
/// SPIR-V formatında derlenir ve GPU'ya iletilir.
/// Vertex shader: köşe noktalarını dönüştürür.
/// Fragment shader: piksel renklerini hesaplar.
/// Compute shader: genel amaçlı GPU hesaplaması (GPGPU).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Geometry,
    TessControl,
    TessEval,
    Compute,
}

#[derive(Clone, Debug)]
pub struct ShaderDesc {
    pub stage: ShaderStage,
    pub code: Vec<u32>, // SPIR-V bayt kodu — assembler benzeri GPU instruction seti
    pub entry_point: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Shader {
    pub handle: ShaderHandle,
    pub desc: ShaderDesc,
}

// ============================================================================
// GRAFİK PIPELINE
// ============================================================================

/// Grafik pipeline — tüm render durumunu (vertex giriş, shader, rasterizasyon,
/// harmanlama) tek bir değişmez nesneye bağlar. Pipeline değiştirilmez;
/// farklı render durumu için yeni pipeline oluşturulur.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
    TriangleFan,
    LineListWithAdjacency,
    LineStripWithAdjacency,
    TriangleListWithAdjacency,
    TriangleStripWithAdjacency,
    PatchList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolygonMode {
    Fill,
    Line,
    Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullMode {
    None,
    Front,
    Back,
    FrontAndBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontFace {
    CounterClockwise,
    Clockwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Never,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilOp {
    Keep,
    Zero,
    Replace,
    IncrementAndClamp,
    DecrementAndClamp,
    Invert,
    IncrementAndWrap,
    DecrementAndWrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcColor,
    OneMinusSrcColor,
    DstColor,
    OneMinusDstColor,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
    ConstantColor,
    OneMinusConstantColor,
    ConstantAlpha,
    OneMinusConstantAlpha,
    SrcAlphaSaturate,
    Src1Color,
    OneMinusSrc1Color,
    Src1Alpha,
    OneMinusSrc1Alpha,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendOp {
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

#[derive(Clone, Debug)]
pub struct StencilOpState {
    pub fail_op: StencilOp,
    pub pass_op: StencilOp,
    pub depth_fail_op: StencilOp,
    pub compare_op: CompareOp,
    pub compare_mask: u32,
    pub write_mask: u32,
    pub reference: u32,
}

#[derive(Clone, Debug)]
pub struct DepthStencilState {
    pub depth_test_enable: bool,
    pub depth_write_enable: bool,
    pub depth_compare_op: CompareOp,
    pub depth_bounds_test_enable: bool,
    pub stencil_test_enable: bool,
    pub front: StencilOpState,
    pub back: StencilOpState,
    pub min_depth_bounds: f32,
    pub max_depth_bounds: f32,
}

#[derive(Clone, Debug)]
pub struct ColorBlendAttachmentState {
    pub blend_enable: bool,
    pub src_color_blend_factor: BlendFactor,
    pub dst_color_blend_factor: BlendFactor,
    pub color_blend_op: BlendOp,
    pub src_alpha_blend_factor: BlendFactor,
    pub dst_alpha_blend_factor: BlendFactor,
    pub alpha_blend_op: BlendOp,
    pub color_write_mask: u8,
}

#[derive(Clone, Debug)]
pub struct ColorBlendState {
    pub logic_op_enable: bool,
    pub logic_op: LogicOp,
    pub attachments: Vec<ColorBlendAttachmentState>,
    pub blend_constants: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicOp {
    Clear,
    And,
    AndReverse,
    Copy,
    AndInverted,
    NoOp,
    Xor,
    Or,
    Nor,
    Equivalent,
    Invert,
    OrReverse,
    CopyInverted,
    OrInverted,
    Nand,
    Set,
}

#[derive(Clone, Debug)]
pub struct VertexInputBinding {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: VertexInputRate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexInputRate {
    Vertex,
    Instance,
}

#[derive(Clone, Debug)]
pub struct VertexInputAttribute {
    pub location: u32,
    pub binding: u32,
    pub format: Format,
    pub offset: u32,
}

#[derive(Clone, Debug)]
pub struct PipelineDesc {
    pub vertex_shader: ShaderHandle,
    pub fragment_shader: Option<ShaderHandle>,
    pub geometry_shader: Option<ShaderHandle>,
    pub topology: PrimitiveTopology,
    pub primitive_restart: bool,
    pub polygon_mode: PolygonMode,
    pub cull_mode: CullMode,
    pub front_face: FrontFace,
    pub depth_bias_enable: bool,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub depth_stencil: DepthStencilState,
    pub color_blend: ColorBlendState,
    pub vertex_bindings: Vec<VertexInputBinding>,
    pub vertex_attributes: Vec<VertexInputAttribute>,
    pub render_pass: RenderPassHandle,
    pub subpass: u32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Pipeline {
    pub handle: PipelineHandle,
    pub desc: PipelineDesc,
}

// ============================================================================
// RENDER GEÇİŞİ (RENDER PASS)
// ============================================================================

/// Render pass — hangi renk/derinlik eklerinin kullanılacağını ve
/// bunların nasıl yükleneceğini/depolanacanğını tanımlar.
/// Alt geçişler (subpass) arası bağımlılıklar da burada belirtilir.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentLoadOp {
    Load,
    Clear,
    DontCare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentStoreOp {
    Store,
    DontCare,
}

#[derive(Clone, Debug)]
pub struct AttachmentDescription {
    pub format: Format,
    pub samples: SampleCount,
    pub load_op: AttachmentLoadOp,
    pub store_op: AttachmentStoreOp,
    pub stencil_load_op: AttachmentLoadOp,
    pub stencil_store_op: AttachmentStoreOp,
    pub initial_layout: ImageLayout,
    pub final_layout: ImageLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleCount {
    Count1,
    Count2,
    Count4,
    Count8,
    Count16,
    Count32,
    Count64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageLayout {
    Undefined,
    General,
    ColorAttachment,
    DepthStencilAttachment,
    DepthStencilReadOnly,
    ShaderReadOnly,
    TransferSrc,
    TransferDst,
    Preinitialized,
    DepthReadOnlyStencilAttachment,
    DepthAttachmentStencilReadOnly,
    DepthAttachment,
    DepthReadOnly,
    StencilAttachment,
    StencilReadOnly,
    ReadOnly,
    Attachment,
    Present,
}

#[derive(Clone, Debug)]
pub struct AttachmentReference {
    pub attachment: u32,
    pub layout: ImageLayout,
}

#[derive(Clone, Debug)]
pub struct SubpassDescription {
    pub color_attachments: Vec<AttachmentReference>,
    pub resolve_attachments: Vec<AttachmentReference>,
    pub depth_stencil_attachment: Option<AttachmentReference>,
    pub input_attachments: Vec<AttachmentReference>,
}

#[derive(Clone, Debug)]
pub struct RenderPassDesc {
    pub attachments: Vec<AttachmentDescription>,
    pub subpasses: Vec<SubpassDescription>,
    pub dependencies: Vec<SubpassDependency>,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct SubpassDependency {
    pub src_subpass: u32,
    pub dst_subpass: u32,
    pub src_stage_mask: PipelineStageFlags,
    pub dst_stage_mask: PipelineStageFlags,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub dependency_flags: DependencyFlags,
}

#[derive(Clone, Copy, Debug)]
pub struct PipelineStageFlags(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct AccessFlags(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct DependencyFlags(pub u32);

#[derive(Clone, Debug)]
pub struct RenderPass {
    pub handle: RenderPassHandle,
    pub desc: RenderPassDesc,
}

// ============================================================================
// ÇERÇEVE TAMPONU (FRAMEBUFFER)
// ============================================================================

/// Framebuffer — render pass'in çizim yaptığı ekler kümesi.
/// ImageView'lere referans tutar; bu sayede farklı framebuffer'lar
/// aynı görüntüye farklı perspektiflerden bakabilir.

#[derive(Clone, Debug)]
pub struct FramebufferDesc {
    pub render_pass: RenderPassHandle,
    pub attachments: Vec<ImageViewHandle>,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Framebuffer {
    pub handle: FramebufferHandle,
    pub desc: FramebufferDesc,
}

// ============================================================================
// KOMUT TAMPONU (COMMAND BUFFER)
// ============================================================================

/// Komut tamponu — GPU komutlarının kayıt edildiği sıralı liste.
/// Komutlar önceden kayıt edilir ve toplu olarak GPU'ya gönderilir.
/// Bu yaklaşım, CPU-GPU iletişim maliyetini azaltır.

#[derive(Clone, Debug)]
pub struct CommandBuffer {
    pub handle: CommandBufferHandle,
    pub commands: Vec<Command>,
    pub recording: bool,
}

#[derive(Clone, Debug)]
pub enum Command {
    BeginRenderPass {
        render_pass: RenderPassHandle,
        framebuffer: FramebufferHandle,
        render_area: Rect2D,
        clear_values: Vec<ClearValue>,
    },
    EndRenderPass,
    BindPipeline {
        pipeline: PipelineHandle,
    },
    BindVertexBuffer {
        binding: u32,
        buffer: BufferHandle,
        offset: u64,
    },
    BindIndexBuffer {
        buffer: BufferHandle,
        offset: u64,
        index_type: IndexType,
    },
    BindDescriptorSet {
        pipeline: PipelineHandle,
        set: DescriptorSetHandle,
        first_set: u32,
    },
    Draw {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    DrawIndexed {
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    },
    DrawIndirect {
        buffer: BufferHandle,
        offset: u64,
        draw_count: u32,
        stride: u32,
    },
    DrawIndexedIndirect {
        buffer: BufferHandle,
        offset: u64,
        draw_count: u32,
        stride: u32,
    },
    Dispatch {
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    },
    CopyBuffer {
        src: BufferHandle,
        dst: BufferHandle,
        regions: Vec<BufferCopy>,
    },
    CopyImage {
        src: ImageHandle,
        dst: ImageHandle,
        regions: Vec<ImageCopy>,
    },
    CopyBufferToImage {
        src: BufferHandle,
        dst: ImageHandle,
        regions: Vec<BufferImageCopy>,
    },
    CopyImageToBuffer {
        src: ImageHandle,
        dst: BufferHandle,
        regions: Vec<BufferImageCopy>,
    },
    BlitImage {
        src: ImageHandle,
        dst: ImageHandle,
        regions: Vec<ImageBlit>,
        filter: Filter,
    },
    SetViewport {
        first_viewport: u32,
        viewports: Vec<Viewport>,
    },
    SetScissor {
        first_scissor: u32,
        scissors: Vec<Rect2D>,
    },
    PushConstants {
        pipeline: PipelineHandle,
        stage: ShaderStage,
        offset: u32,
        data: Vec<u8>,
    },
    SetDepthBias {
        constant_factor: f32,
        clamp: f32,
        slope_factor: f32,
    },
    SetBlendConstants {
        constants: [f32; 4],
    },
    SetDepthBounds {
        min: f32,
        max: f32,
    },
    SetStencilCompareMask {
        face: StencilFace,
        mask: u32,
    },
    SetStencilWriteMask {
        face: StencilFace,
        mask: u32,
    },
    SetStencilReference {
        face: StencilFace,
        reference: u32,
    },
    PipelineBarrier {
        src_stage: PipelineStageFlags,
        dst_stage: PipelineStageFlags,
        memory_barriers: Vec<MemoryBarrier>,
        buffer_barriers: Vec<BufferMemoryBarrier>,
        image_barriers: Vec<ImageMemoryBarrier>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexType {
    Uint16,
    Uint32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilFace {
    Front,
    Back,
    FrontAndBack,
}

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect2D {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClearValue {
    pub color: ClearColorValue,
    pub depth_stencil: ClearDepthStencilValue,
}

#[derive(Clone, Copy, Debug)]
pub struct ClearColorValue {
    pub float32: [f32; 4],
    pub int32: [i32; 4],
    pub uint32: [u32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct ClearDepthStencilValue {
    pub depth: f32,
    pub stencil: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct BufferCopy {
    pub src_offset: u64,
    pub dst_offset: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageCopy {
    pub src_subresource: ImageSubresourceLayers,
    pub src_offset: Offset3D,
    pub dst_subresource: ImageSubresourceLayers,
    pub dst_offset: Offset3D,
    pub extent: Extent3D,
}

#[derive(Clone, Copy, Debug)]
pub struct BufferImageCopy {
    pub buffer_offset: u64,
    pub buffer_row_length: u32,
    pub buffer_image_height: u32,
    pub image_subresource: ImageSubresourceLayers,
    pub image_offset: Offset3D,
    pub image_extent: Extent3D,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageBlit {
    pub src_subresource: ImageSubresourceLayers,
    pub src_offsets: [Offset3D; 2],
    pub dst_subresource: ImageSubresourceLayers,
    pub dst_offsets: [Offset3D; 2],
}

#[derive(Clone, Copy, Debug)]
pub struct ImageSubresourceLayers {
    pub aspect_mask: ImageAspectFlags,
    pub mip_level: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageAspectFlags(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct Offset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[derive(Clone, Debug)]
pub struct MemoryBarrier {
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
}

#[derive(Clone, Debug)]
pub struct BufferMemoryBarrier {
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub buffer: BufferHandle,
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct ImageMemoryBarrier {
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub old_layout: ImageLayout,
    pub new_layout: ImageLayout,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub image: ImageHandle,
    pub subresource_range: ImageSubresourceRange,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageSubresourceRange {
    pub aspect_mask: ImageAspectFlags,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

// ============================================================================
// TANIMLAYICI KÜMESİ (DESCRIPTOR SET)
// ============================================================================

/// Descriptor set — shader kaynaklarını (tampon, doku, örnekleyici) bağlar.
/// Set düzeni, shader kodundaki binding numaraları ile eşleşmelidir.
/// Vulkan descriptor sistemi, kaynakların yeniden kullanımını verimli kılar.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DescriptorType {
    Sampler,
    CombinedImageSampler,
    SampledImage,
    StorageImage,
    UniformTexelBuffer,
    StorageTexelBuffer,
    UniformBuffer,
    StorageBuffer,
    UniformBufferDynamic,
    StorageBufferDynamic,
    InputAttachment,
}

#[derive(Clone, Debug)]
pub struct DescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: DescriptorType,
    pub descriptor_count: u32,
    pub stage_flags: ShaderStageFlags,
    pub immutable_samplers: Vec<SamplerHandle>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShaderStageFlags(pub u32);

#[derive(Clone, Debug)]
pub struct DescriptorSet {
    pub handle: DescriptorSetHandle,
    pub bindings: BTreeMap<u32, DescriptorBinding>,
}

#[derive(Clone, Debug)]
pub enum DescriptorBinding {
    Sampler(SamplerHandle),
    CombinedImageSampler(ImageHandle, SamplerHandle),
    SampledImage(ImageViewHandle),
    StorageImage(ImageViewHandle),
    UniformBuffer(BufferHandle, u64, u64),
    StorageBuffer(BufferHandle, u64, u64),
}

// ============================================================================
// SENKRONIZASYON NESNELERİ
// ============================================================================

/// Fence — CPU ve GPU arasında senkronizasyon.
/// CPU, fence sinyal verene kadar bekleyebilir.
/// Semaphore ise GPU-GPU senkronizasyonu için kullanılır.

#[derive(Clone, Debug)]
pub struct Fence {
    pub handle: FenceHandle,
    pub signaled: bool,
    pub current_value: u64,
    pub monitored_value: u64,
    pub signal_count: u64,
    pub last_signal_tick: u64,
    pub log: Vec<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FenceSnapshot {
    pub current_value: u64,
    pub monitored_value: u64,
    pub signaled: bool,
    pub signal_count: u64,
    pub last_signal_tick: u64,
}

#[derive(Clone, Debug)]
pub struct Semaphore {
    pub handle: SemaphoreHandle,
    pub signaled: bool,
}

// ============================================================================
// GPU CİHAZI (GPU DEVICE)
// ============================================================================

/// GPU cihazı — tüm GPU kaynaklarını yöneten merkezi yapı.
/// BTreeMap kullanılır: O(log n) erişim, deterministik sıralama.
/// Kaynak handle'larını gerçek nesnelere eşler.

#[derive(Clone, Debug)]
pub struct GpuDevice {
    pub buffers: BTreeMap<BufferHandle, Buffer>,
    pub images: BTreeMap<ImageHandle, Image>,
    pub image_views: BTreeMap<ImageViewHandle, ImageView>,
    pub samplers: BTreeMap<SamplerHandle, Sampler>,
    pub shaders: BTreeMap<ShaderHandle, Shader>,
    pub pipelines: BTreeMap<PipelineHandle, Pipeline>,
    pub render_passes: BTreeMap<RenderPassHandle, RenderPass>,
    pub framebuffers: BTreeMap<FramebufferHandle, Framebuffer>,
    pub command_buffers: BTreeMap<CommandBufferHandle, CommandBuffer>,
    pub descriptor_sets: BTreeMap<DescriptorSetHandle, DescriptorSet>,
    pub fences: BTreeMap<FenceHandle, Fence>,
    pub semaphores: BTreeMap<SemaphoreHandle, Semaphore>,
    pub shader_cache: BTreeMap<String, ShaderHandle>,
    pub pipeline_cache: BTreeMap<String, PipelineHandle>,
    pub render_pass_cache: BTreeMap<String, RenderPassHandle>,
    pub fence_registry: BTreeMap<String, FenceHandle>,
    pub next_handle: u64,
    pub name: String,
}

impl GpuDevice {
    pub fn new(name: &str) -> Self {
        GpuDevice {
            buffers: BTreeMap::new(),
            images: BTreeMap::new(),
            image_views: BTreeMap::new(),
            samplers: BTreeMap::new(),
            shaders: BTreeMap::new(),
            pipelines: BTreeMap::new(),
            render_passes: BTreeMap::new(),
            framebuffers: BTreeMap::new(),
            command_buffers: BTreeMap::new(),
            descriptor_sets: BTreeMap::new(),
            fences: BTreeMap::new(),
            semaphores: BTreeMap::new(),
            shader_cache: BTreeMap::new(),
            pipeline_cache: BTreeMap::new(),
            render_pass_cache: BTreeMap::new(),
            fence_registry: BTreeMap::new(),
            next_handle: 1,
            name: name.to_string(),
        }
    }

    fn next_handle(&mut self) -> u64 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    pub fn create_buffer(&mut self, desc: BufferDesc) -> BufferHandle {
        let handle = BufferHandle(self.next_handle());
        let buffer = Buffer::new(handle, desc);
        self.buffers.insert(handle, buffer);
        handle
    }

    pub fn create_image(&mut self, desc: ImageDesc) -> ImageHandle {
        let handle = ImageHandle(self.next_handle());
        let image = Image::new(handle, desc);
        self.images.insert(handle, image);
        handle
    }

    pub fn create_shader(&mut self, desc: ShaderDesc) -> ShaderHandle {
        let handle = ShaderHandle(self.next_handle());
        let shader = Shader { handle, desc };
        self.shaders.insert(handle, shader);
        handle
    }

    pub fn create_render_pass(&mut self, desc: RenderPassDesc) -> RenderPassHandle {
        let handle = RenderPassHandle(self.next_handle());
        let pass = RenderPass { handle, desc };
        self.render_passes.insert(handle, pass);
        handle
    }

    pub fn create_pipeline(&mut self, desc: PipelineDesc) -> PipelineHandle {
        let handle = PipelineHandle(self.next_handle());
        let pipeline = Pipeline { handle, desc };
        self.pipelines.insert(handle, pipeline);
        handle
    }

    pub fn cache_shader(&mut self, key: &str, desc: ShaderDesc) -> (ShaderHandle, bool) {
        if let Some(handle) = self.shader_cache.get(key).copied() {
            return (handle, true);
        }
        let handle = self.create_shader(desc);
        self.shader_cache.insert(key.to_string(), handle);
        (handle, false)
    }

    pub fn cache_render_pass(&mut self, key: &str, desc: RenderPassDesc) -> RenderPassHandle {
        if let Some(handle) = self.render_pass_cache.get(key).copied() {
            return handle;
        }
        let handle = self.create_render_pass(desc);
        self.render_pass_cache.insert(key.to_string(), handle);
        handle
    }

    pub fn cache_pipeline(&mut self, key: &str, desc: PipelineDesc) -> (PipelineHandle, bool) {
        if let Some(handle) = self.pipeline_cache.get(key).copied() {
            return (handle, true);
        }
        let handle = self.create_pipeline(desc);
        self.pipeline_cache.insert(key.to_string(), handle);
        (handle, false)
    }

    pub fn register_named_fence(&mut self, name: &str, signaled: bool) -> FenceHandle {
        if let Some(handle) = self.fence_registry.get(name).copied() {
            if let Some(fence) = self.fences.get_mut(&handle) {
                if signaled {
                    let next_value = fence.current_value.saturating_add(1).max(1);
                    Self::update_fence_value(fence, next_value);
                } else {
                    fence.monitored_value = fence.current_value.saturating_add(1);
                    fence.signaled = false;
                }
            }
            return handle;
        }
        let handle = FenceHandle(self.next_handle());
        let initial_value = if signaled { 1 } else { 0 };
        self.fences.insert(
            handle,
            Fence {
                handle,
                signaled,
                current_value: initial_value,
                monitored_value: initial_value,
                signal_count: signaled as u64,
                last_signal_tick: crate::interrupts::get_ticks(),
                log: if signaled {
                    vec![initial_value]
                } else {
                    Vec::new()
                },
            },
        );
        self.fence_registry.insert(name.to_string(), handle);
        handle
    }

    fn update_fence_value(fence: &mut Fence, value: u64) {
        if value > fence.current_value {
            fence.current_value = value;
            fence.signal_count = fence.signal_count.saturating_add(1);
            fence.last_signal_tick = crate::interrupts::get_ticks();
            if fence.log.len() >= 64 {
                let _ = fence.log.remove(0);
            }
            fence.log.push(value);
        }
        fence.signaled = fence.current_value >= fence.monitored_value;
    }

    pub fn set_fence_state(&mut self, handle: FenceHandle, signaled: bool) -> Result<(), GpuError> {
        let Some(fence) = self.fences.get_mut(&handle) else {
            return Err(GpuError::InvalidHandle);
        };
        if signaled {
            let next_value = fence
                .current_value
                .saturating_add(1)
                .max(fence.monitored_value.max(1));
            Self::update_fence_value(fence, next_value);
        } else {
            fence.monitored_value = fence.current_value.saturating_add(1);
            fence.signaled = false;
        }
        Ok(())
    }

    pub fn fence_state(&self, handle: FenceHandle) -> Option<bool> {
        self.fences.get(&handle).map(|fence| fence.signaled)
    }

    pub fn signal_fence_value(&mut self, handle: FenceHandle, value: u64) -> Result<(), GpuError> {
        let Some(fence) = self.fences.get_mut(&handle) else {
            return Err(GpuError::InvalidHandle);
        };
        Self::update_fence_value(fence, value);
        Ok(())
    }

    pub fn set_fence_target(&mut self, handle: FenceHandle, value: u64) -> Result<(), GpuError> {
        let Some(fence) = self.fences.get_mut(&handle) else {
            return Err(GpuError::InvalidHandle);
        };
        fence.monitored_value = value.max(fence.current_value);
        fence.signaled = fence.current_value >= fence.monitored_value;
        Ok(())
    }

    pub fn fence_value(&self, handle: FenceHandle) -> Option<u64> {
        self.fences.get(&handle).map(|fence| fence.current_value)
    }

    pub fn fence_snapshot(&self, handle: FenceHandle) -> Option<FenceSnapshot> {
        self.fences.get(&handle).map(|fence| FenceSnapshot {
            current_value: fence.current_value,
            monitored_value: fence.monitored_value,
            signaled: fence.signaled,
            signal_count: fence.signal_count,
            last_signal_tick: fence.last_signal_tick,
        })
    }

    pub fn fence_log(&self, handle: FenceHandle) -> Option<Vec<u64>> {
        self.fences.get(&handle).map(|fence| fence.log.clone())
    }

    pub fn create_framebuffer(&mut self, desc: FramebufferDesc) -> FramebufferHandle {
        let handle = FramebufferHandle(self.next_handle());
        let fb = Framebuffer { handle, desc };
        self.framebuffers.insert(handle, fb);
        handle
    }

    pub fn create_command_buffer(&mut self) -> CommandBufferHandle {
        let handle = CommandBufferHandle(self.next_handle());
        let cmd = CommandBuffer {
            handle,
            commands: Vec::new(),
            recording: false,
        };
        self.command_buffers.insert(handle, cmd);
        handle
    }

    pub fn get_buffer(&self, handle: BufferHandle) -> Option<&Buffer> {
        self.buffers.get(&handle)
    }

    pub fn get_buffer_mut(&mut self, handle: BufferHandle) -> Option<&mut Buffer> {
        self.buffers.get_mut(&handle)
    }

    pub fn get_image(&self, handle: ImageHandle) -> Option<&Image> {
        self.images.get(&handle)
    }

    pub fn get_image_mut(&mut self, handle: ImageHandle) -> Option<&mut Image> {
        self.images.get_mut(&handle)
    }

    pub fn destroy_buffer(&mut self, handle: BufferHandle) {
        self.buffers.remove(&handle);
    }

    pub fn destroy_image(&mut self, handle: ImageHandle) {
        self.images.remove(&handle);
    }
}

impl Default for GpuDevice {
    fn default() -> Self {
        Self::new("default")
    }
}

// ============================================================================
// GLOBAL GPU ÖRNEĞİ
// ============================================================================

/// Global GPU aygıtı — lazy_static ile ilk erişimde başlatılır.
/// Mutex ile çoklu iş parçacıklı erişim güvence altına alınır.

lazy_static::lazy_static! {
    static ref GPU_DEVICE: Mutex<GpuDevice> = Mutex::new(GpuDevice::new("default"));
}

/// GPU 3D alt sistemini başlatır. Çekirdek başlatma sırasında çağrılır.
pub fn init() {
    crate::serial_println!("[GPU3D] 3D grafik API'si başlatıldı");
}

pub fn cache_shader_program(key: &str, desc: ShaderDesc) -> (ShaderHandle, bool) {
    GPU_DEVICE.lock().cache_shader(key, desc)
}

pub fn create_cached_render_pass(key: &str, desc: RenderPassDesc) -> RenderPassHandle {
    GPU_DEVICE.lock().cache_render_pass(key, desc)
}

pub fn cache_pipeline_state(key: &str, desc: PipelineDesc) -> (PipelineHandle, bool) {
    GPU_DEVICE.lock().cache_pipeline(key, desc)
}

pub fn register_named_fence(name: &str, signaled: bool) -> FenceHandle {
    GPU_DEVICE.lock().register_named_fence(name, signaled)
}

pub fn set_fence_state(handle: FenceHandle, signaled: bool) -> Result<(), GpuError> {
    GPU_DEVICE.lock().set_fence_state(handle, signaled)
}

pub fn fence_state(handle: FenceHandle) -> Option<bool> {
    GPU_DEVICE.lock().fence_state(handle)
}

pub fn signal_fence_value(handle: FenceHandle, value: u64) -> Result<(), GpuError> {
    GPU_DEVICE.lock().signal_fence_value(handle, value)
}

pub fn set_fence_target(handle: FenceHandle, value: u64) -> Result<(), GpuError> {
    GPU_DEVICE.lock().set_fence_target(handle, value)
}

pub fn fence_value(handle: FenceHandle) -> Option<u64> {
    GPU_DEVICE.lock().fence_value(handle)
}

pub fn fence_snapshot(handle: FenceHandle) -> Option<FenceSnapshot> {
    GPU_DEVICE.lock().fence_snapshot(handle)
}

pub fn fence_log(handle: FenceHandle) -> Option<Vec<u64>> {
    GPU_DEVICE.lock().fence_log(handle)
}

/// Yeni bir GPU tamponu oluşturur ve handle'ını döndürür.
/// Tampon içeriği başlangıçta sıfırlanmış olarak gelir.
pub fn create_buffer(desc: BufferDesc) -> BufferHandle {
    GPU_DEVICE.lock().create_buffer(desc)
}

/// Yeni bir GPU görüntüsü (texture) oluşturur ve handle'ını döndürür.
pub fn create_image(desc: ImageDesc) -> ImageHandle {
    GPU_DEVICE.lock().create_image(desc)
}

/// Yeni bir shader programı oluşturur ve handle'ını döndürür.
/// SPIR-V bayt kodu ShaderDesc.code içinde sağlanmalıdır.
pub fn create_shader(desc: ShaderDesc) -> ShaderHandle {
    GPU_DEVICE.lock().create_shader(desc)
}

/// Yeni bir render pass oluşturur ve handle'ını döndürür.
pub fn create_render_pass(desc: RenderPassDesc) -> RenderPassHandle {
    GPU_DEVICE.lock().create_render_pass(desc)
}

/// Yeni bir grafik pipeline oluşturur ve handle'ını döndürür.
pub fn create_pipeline(desc: PipelineDesc) -> PipelineHandle {
    GPU_DEVICE.lock().create_pipeline(desc)
}

/// Yeni bir framebuffer oluşturur ve handle'ını döndürür.
pub fn create_framebuffer(desc: FramebufferDesc) -> FramebufferHandle {
    GPU_DEVICE.lock().create_framebuffer(desc)
}

/// Yeni bir komut tamponu oluşturur.
/// Komutlar begin_command_buffer() sonrası kayıt edilir.
pub fn create_command_buffer() -> CommandBufferHandle {
    GPU_DEVICE.lock().create_command_buffer()
}

/// Handle ile tamponu sorgular; bulunamazsa None döner.
pub fn get_buffer(handle: BufferHandle) -> Option<Buffer> {
    GPU_DEVICE.lock().get_buffer(handle).cloned()
}

/// Handle ile görüntüyü sorgular; bulunamazsa None döner.
pub fn get_image(handle: ImageHandle) -> Option<Image> {
    GPU_DEVICE.lock().get_image(handle).cloned()
}

/// Belirtilen ofsetten başlayarak tampona veri yazar.
pub fn write_buffer(handle: BufferHandle, offset: u64, data: &[u8]) -> Result<(), GpuError> {
    GPU_DEVICE
        .lock()
        .get_buffer_mut(handle)
        .ok_or(GpuError::InvalidHandle)?
        .write(offset, data)
}

/// Tamponu yok eder ve GPU belleğini serbest bırakır.
pub fn destroy_buffer(handle: BufferHandle) {
    GPU_DEVICE.lock().destroy_buffer(handle)
}

/// Görüntüyü yok eder ve GPU belleğini serbest bırakır.
pub fn destroy_image(handle: ImageHandle) {
    GPU_DEVICE.lock().destroy_image(handle)
}
