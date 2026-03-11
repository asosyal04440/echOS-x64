//! # FAZ VIII Ecosystem Coordinator
//!
//! Ekosistem fazındaki uyumluluk, güncelleme ve gözlemlenebilirlik ihtiyaçlarını
//! tek bir koordinatörde birleştirir:
//! - Win32/eLS yürütme yollarında IronShim + Valkyrie-V zorunlu politika geçidi
//! - PE/ELF hazırlık akışları
//! - A/B atomik güncelleme + delta patch
//! - bpftrace uyumlu kprobe/uprobe kayıt yolu
//! - vmcore + minidump üretim köprüsü

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::debug::kdump;
use crate::ebpf::{self, BpfAttachPoint, BpfError};
use crate::elf::{parse_elf, ElfError, ElfImage};
use crate::gpu3d::{
    self, AccessFlags, AttachmentDescription, AttachmentLoadOp, AttachmentReference,
    AttachmentStoreOp, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
    CompareOp, CullMode, DependencyFlags, DepthStencilState, FenceHandle, FrontFace, Format,
    ImageLayout, LogicOp, PipelineDesc, PipelineStageFlags, PolygonMode, PrimitiveTopology,
    RenderPassDesc, ShaderDesc, ShaderStage, StencilOp, StencilOpState, SubpassDependency,
    SubpassDescription, VertexInputRate,
};
use crate::gui::protocol::{
    CompositorPass, DamageTile, DisplayPresentMode, FrameIntent, Point, Rect, ScanoutCandidate,
    SurfaceTransform, SurfaceId, VblankFeedback,
};
use crate::pe_loader::{load_pe, PeError, PeImage};
use crate::services::ech_display::{DisplayCommand, DisplayResponse};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubsystemTarget {
    Win32,
    Els,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskTier {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsolationPlan {
    pub requires_ironshim: bool,
    pub requires_valkyrie: bool,
    pub domain_label: &'static str,
}

#[derive(Clone, Debug)]
pub enum EcosystemError {
    IsolationUnavailable,
    InvalidProbeSpec,
    UpdateStateInvalid,
    DeltaHashMismatch,
    NoStagedPatch,
    Pe(PeError),
    Elf(ElfError),
    Ebpf(BpfError),
}

impl From<PeError> for EcosystemError {
    fn from(value: PeError) -> Self {
        Self::Pe(value)
    }
}

impl From<ElfError> for EcosystemError {
    fn from(value: ElfError) -> Self {
        Self::Elf(value)
    }
}

impl From<BpfError> for EcosystemError {
    fn from(value: BpfError) -> Self {
        Self::Ebpf(value)
    }
}

#[derive(Clone, Debug)]
pub struct Win32PreparedImage {
    pub image: PeImage,
    pub isolation: IsolationPlan,
    pub translated_modules: Vec<String>,
    pub dxvk_targets: Vec<String>,
    pub abi_modules: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsBackend {
    Vulkan,
}

impl GraphicsBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxgiSurfaceDesc {
    pub surface_id: SurfaceId,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub api: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxgiPresentRequest {
    pub api: String,
    pub width: u32,
    pub height: u32,
    pub damage: Rect,
    pub opaque: bool,
    pub present_mode: DisplayPresentMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxgiPresentResult {
    pub present_id: u64,
    pub surface: DxgiSurfaceDesc,
    pub backend: GraphicsBackend,
    pub shader_cache_hit: bool,
    pub pipeline_cache_hit: bool,
    pub fence: FenceHandle,
    pub feedback: Option<VblankFeedback>,
}

pub struct DxgiPresentBridge {
    targets: BTreeMap<String, GraphicsBackend>,
    next_surface_id: AtomicU64,
    next_present_id: AtomicU64,
}

impl DxgiPresentBridge {
    pub fn new() -> Self {
        let mut targets = BTreeMap::new();
        targets.insert(String::from("d3d9"), GraphicsBackend::Vulkan);
        targets.insert(String::from("d3d11"), GraphicsBackend::Vulkan);
        targets.insert(String::from("d3d12"), GraphicsBackend::Vulkan);
        targets.insert(String::from("dxgi"), GraphicsBackend::Vulkan);
        Self {
            targets,
            next_surface_id: AtomicU64::new(0x4000),
            next_present_id: AtomicU64::new(1),
        }
    }

    pub fn target_for(&self, api: &str) -> Option<GraphicsBackend> {
        self.targets.get(&api.to_lowercase()).copied()
    }

    pub fn describe_targets(&self) -> Vec<String> {
        self.targets
            .iter()
            .map(|(api, backend)| format!("{}=>{}", api, backend.as_str()))
            .collect()
    }

    fn build_surface(&self, request: &DxgiPresentRequest) -> DxgiSurfaceDesc {
        DxgiSurfaceDesc {
            surface_id: self.next_surface_id.fetch_add(1, Ordering::Relaxed),
            width: request.width,
            height: request.height,
            format: Format::B8G8R8A8Unorm,
            api: request.api.to_lowercase(),
        }
    }

    fn prime_gpu_contract(
        &self,
        request: &DxgiPresentRequest,
    ) -> Result<(bool, bool, FenceHandle), EcosystemError> {
        let backend = self
            .target_for(&request.api)
            .ok_or(EcosystemError::InvalidProbeSpec)?;
        let shader_prefix = format!("dxgi:{}:{}", request.api.to_lowercase(), backend.as_str());
        let (vertex_shader, shader_cache_hit_a) = gpu3d::cache_shader_program(
            &format!("{}:vs", shader_prefix),
            ShaderDesc {
                stage: ShaderStage::Vertex,
                code: vec![0x07230203, 0x00010000, 0x000d0003, 0x00000002],
                entry_point: String::from("main"),
                name: format!("{}-vs", shader_prefix),
            },
        );
        let (fragment_shader, shader_cache_hit_b) = gpu3d::cache_shader_program(
            &format!("{}:fs", shader_prefix),
            ShaderDesc {
                stage: ShaderStage::Fragment,
                code: vec![0x07230203, 0x00010000, 0x000d0003, 0x00000002],
                entry_point: String::from("main"),
                name: format!("{}-fs", shader_prefix),
            },
        );
        let render_pass = gpu3d::create_cached_render_pass(
            &format!("{}:render-pass", shader_prefix),
            RenderPassDesc {
                attachments: vec![AttachmentDescription {
                    format: Format::B8G8R8A8Unorm,
                    samples: gpu3d::SampleCount::Count1,
                    load_op: AttachmentLoadOp::Load,
                    store_op: AttachmentStoreOp::Store,
                    stencil_load_op: AttachmentLoadOp::DontCare,
                    stencil_store_op: AttachmentStoreOp::DontCare,
                    initial_layout: ImageLayout::ColorAttachment,
                    final_layout: ImageLayout::Present,
                }],
                subpasses: vec![SubpassDescription {
                    color_attachments: vec![AttachmentReference {
                        attachment: 0,
                        layout: ImageLayout::ColorAttachment,
                    }],
                    resolve_attachments: Vec::new(),
                    depth_stencil_attachment: None,
                    input_attachments: Vec::new(),
                }],
                dependencies: vec![SubpassDependency {
                    src_subpass: 0,
                    dst_subpass: 0,
                    src_stage_mask: PipelineStageFlags(1),
                    dst_stage_mask: PipelineStageFlags(1),
                    src_access_mask: AccessFlags(1),
                    dst_access_mask: AccessFlags(1),
                    dependency_flags: DependencyFlags(0),
                }],
                name: format!("{}-rp", shader_prefix),
            },
        );
        let (_, pipeline_cache_hit) = gpu3d::cache_pipeline_state(
            &format!("{}:pipeline", shader_prefix),
            PipelineDesc {
                vertex_shader,
                fragment_shader: Some(fragment_shader),
                geometry_shader: None,
                topology: PrimitiveTopology::TriangleStrip,
                primitive_restart: false,
                polygon_mode: PolygonMode::Fill,
                cull_mode: CullMode::Back,
                front_face: FrontFace::CounterClockwise,
                depth_bias_enable: false,
                depth_bias_constant_factor: 0.0,
                depth_bias_clamp: 0.0,
                depth_bias_slope_factor: 0.0,
                depth_stencil: DepthStencilState {
                    depth_test_enable: false,
                    depth_write_enable: false,
                    depth_compare_op: CompareOp::Always,
                    depth_bounds_test_enable: false,
                    stencil_test_enable: false,
                    front: StencilOpState {
                        fail_op: StencilOp::Keep,
                        pass_op: StencilOp::Keep,
                        depth_fail_op: StencilOp::Keep,
                        compare_op: CompareOp::Always,
                        compare_mask: 0,
                        write_mask: 0,
                        reference: 0,
                    },
                    back: StencilOpState {
                        fail_op: StencilOp::Keep,
                        pass_op: StencilOp::Keep,
                        depth_fail_op: StencilOp::Keep,
                        compare_op: CompareOp::Always,
                        compare_mask: 0,
                        write_mask: 0,
                        reference: 0,
                    },
                    min_depth_bounds: 0.0,
                    max_depth_bounds: 1.0,
                },
                color_blend: ColorBlendState {
                    logic_op_enable: false,
                    logic_op: LogicOp::NoOp,
                    attachments: vec![ColorBlendAttachmentState {
                        blend_enable: false,
                        src_color_blend_factor: BlendFactor::One,
                        dst_color_blend_factor: BlendFactor::Zero,
                        color_blend_op: BlendOp::Add,
                        src_alpha_blend_factor: BlendFactor::One,
                        dst_alpha_blend_factor: BlendFactor::Zero,
                        alpha_blend_op: BlendOp::Add,
                        color_write_mask: 0xF,
                    }],
                    blend_constants: [0.0; 4],
                },
                vertex_bindings: vec![gpu3d::VertexInputBinding {
                    binding: 0,
                    stride: 16,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: Vec::new(),
                render_pass,
                subpass: 0,
                name: format!("{}-pipe", shader_prefix),
            },
        );
        let fence = gpu3d::register_named_fence(&format!("{}:present-fence", shader_prefix), false);
        Ok((shader_cache_hit_a && shader_cache_hit_b, pipeline_cache_hit, fence))
    }

    pub fn present(&self, request: DxgiPresentRequest) -> Result<DxgiPresentResult, EcosystemError> {
        let backend = self
            .target_for(&request.api)
            .ok_or(EcosystemError::InvalidProbeSpec)?;
        let surface = self.build_surface(&request);
        let (shader_cache_hit, pipeline_cache_hit, fence) = self.prime_gpu_contract(&request)?;
        let present_id = self.next_present_id.fetch_add(1, Ordering::Relaxed);
        let _ = gpu3d::set_fence_target(fence, present_id);

        let intent = FrameIntent {
            frame_id: present_id,
            enqueue_timestamp_ns: crate::cpu::tsc::read_ns(),
            damage_tiles: vec![DamageTile {
                x: request.damage.x.max(0) as u16,
                y: request.damage.y.max(0) as u16,
                width: request.damage.width.min(u16::MAX as u32) as u16,
                height: request.damage.height.min(u16::MAX as u32) as u16,
            }],
            candidates: vec![ScanoutCandidate {
                surface_id: surface.surface_id,
                z: 0,
                opaque: request.opaque,
                transform: SurfaceTransform::Identity,
                damage: request.damage,
            }],
            composed_passes: vec![CompositorPass::BaseComposite],
            target_refresh_hz: match request.present_mode {
                DisplayPresentMode::Mailbox => 240,
                DisplayPresentMode::VblankFifo => 120,
                DisplayPresentMode::AdaptiveSync => 240,
            },
            mode: request.present_mode,
            cursor_position: Some(Point::new(0, 0)),
        };

        let feedback = crate::services::ech_display::get_display()
            .lock()
            .clone()
            .and_then(|display| {
                match display.process_command(DisplayCommand::SubmitFrameIntent { intent }) {
                    DisplayResponse::Presented { feedback, .. } => Some(feedback),
                    _ => None,
                }
            });
        if let Some(feedback) = feedback {
            let _ = gpu3d::signal_fence_value(fence, feedback.presented_frame_id);
        }

        Ok(DxgiPresentResult {
            present_id,
            surface,
            backend,
            shader_cache_hit,
            pipeline_cache_hit,
            fence,
            feedback,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ElsPreparedImage {
    pub image: ElfImage,
    pub isolation: IsolationPlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpdateSlot {
    A,
    B,
}

impl UpdateSlot {
    fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeltaChunk {
    pub offset: u32,
    pub remove_len: u32,
    pub insert: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryDeltaPatch {
    pub from_hash: u64,
    pub to_hash: u64,
    pub chunks: Vec<DeltaChunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCommitReport {
    pub new_active_slot: UpdateSlot,
    pub generation: u64,
    pub image_bytes: usize,
}

pub struct AtomicUpdateManager {
    active_slot: UpdateSlot,
    slot_images: BTreeMap<UpdateSlot, Vec<u8>>,
    generations: BTreeMap<UpdateSlot, u64>,
    staged_patch: Option<BinaryDeltaPatch>,
}

impl AtomicUpdateManager {
    pub fn new() -> Self {
        let mut slot_images = BTreeMap::new();
        slot_images.insert(UpdateSlot::A, Vec::new());
        slot_images.insert(UpdateSlot::B, Vec::new());

        let mut generations = BTreeMap::new();
        generations.insert(UpdateSlot::A, 0);
        generations.insert(UpdateSlot::B, 0);

        Self {
            active_slot: UpdateSlot::A,
            slot_images,
            generations,
            staged_patch: None,
        }
    }

    pub fn active_slot(&self) -> UpdateSlot {
        self.active_slot
    }

    pub fn active_image(&self) -> &[u8] {
        self.slot_images
            .get(&self.active_slot)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn set_slot_image(&mut self, slot: UpdateSlot, image: Vec<u8>) {
        self.slot_images.insert(slot, image);
    }

    pub fn stage_patch(&mut self, patch: BinaryDeltaPatch) {
        self.staged_patch = Some(patch);
    }

    /// Tek-parça delta üretimi:
    /// `delta = middle(to) - middle(from)` (LCP/LCS ortak bölgeler korunur)
    pub fn build_delta(from: &[u8], to: &[u8]) -> BinaryDeltaPatch {
        let from_hash = fnv1a64(from);
        let to_hash = fnv1a64(to);

        let mut lcp = 0usize;
        while lcp < from.len() && lcp < to.len() && from[lcp] == to[lcp] {
            lcp += 1;
        }

        let mut lcs = 0usize;
        while lcs < (from.len().saturating_sub(lcp))
            && lcs < (to.len().saturating_sub(lcp))
            && from[from.len() - 1 - lcs] == to[to.len() - 1 - lcs]
        {
            lcs += 1;
        }

        let remove_len = from.len().saturating_sub(lcp).saturating_sub(lcs);
        let insert_start = lcp;
        let insert_end = to.len().saturating_sub(lcs);
        let insert = if insert_end > insert_start {
            to[insert_start..insert_end].to_vec()
        } else {
            Vec::new()
        };

        BinaryDeltaPatch {
            from_hash,
            to_hash,
            chunks: vec![DeltaChunk {
                offset: lcp as u32,
                remove_len: remove_len as u32,
                insert,
            }],
        }
    }

    pub fn apply_delta(base: &[u8], patch: &BinaryDeltaPatch) -> Result<Vec<u8>, EcosystemError> {
        if fnv1a64(base) != patch.from_hash {
            return Err(EcosystemError::DeltaHashMismatch);
        }
        let mut output = base.to_vec();

        for chunk in &patch.chunks {
            let offset = chunk.offset as usize;
            let remove_len = chunk.remove_len as usize;
            if offset > output.len() || offset.saturating_add(remove_len) > output.len() {
                return Err(EcosystemError::UpdateStateInvalid);
            }
            output.splice(offset..offset + remove_len, chunk.insert.clone());
        }

        if fnv1a64(&output) != patch.to_hash {
            return Err(EcosystemError::DeltaHashMismatch);
        }
        Ok(output)
    }

    pub fn commit_staged_patch(&mut self) -> Result<UpdateCommitReport, EcosystemError> {
        let patch = self.staged_patch.clone().ok_or(EcosystemError::NoStagedPatch)?;
        let active_image = self.active_image().to_vec();
        let next_image = Self::apply_delta(&active_image, &patch)?;
        let next_slot = self.active_slot.other();
        self.slot_images.insert(next_slot, next_image.clone());
        self.active_slot = next_slot;

        let current_gen = self.generations.get(&next_slot).copied().unwrap_or(0);
        let next_gen = current_gen.saturating_add(1);
        self.generations.insert(next_slot, next_gen);
        self.staged_patch = None;

        Ok(UpdateCommitReport {
            new_active_slot: next_slot,
            generation: next_gen,
            image_bytes: next_image.len(),
        })
    }
}

impl Default for AtomicUpdateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BhdPermissionManifest {
    pub capabilities: Vec<String>,
    pub signature_required: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeKind {
    Kprobe,
    Uprobe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredProbe {
    pub id: u64,
    pub kind: ProbeKind,
    pub spec: String,
    pub program_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpArtifacts {
    pub vmcore: Vec<u8>,
    pub minidump: Vec<u8>,
}

pub struct EcosystemCoordinator {
    ironshim_ready: bool,
    valkyrie_ready: bool,
    dll_translation: BTreeMap<String, String>,
    dxgi_bridge: DxgiPresentBridge,
    updates: AtomicUpdateManager,
    probes: BTreeMap<u64, RegisteredProbe>,
    next_probe_id: AtomicU64,
}

impl EcosystemCoordinator {
    pub fn new() -> Self {
        let mut dll_translation = BTreeMap::new();
        dll_translation.insert(String::from("kernel32"), String::from("echos.win32.kernel"));
        dll_translation.insert(String::from("user32"), String::from("echos.win32.ui"));
        dll_translation.insert(String::from("gdi32"), String::from("echos.win32.gfx"));
        dll_translation.insert(String::from("ntdll"), String::from("echos.win32.ntabi"));
        dll_translation.insert(String::from("ws2_32"), String::from("echos.win32.net"));

        Self {
            ironshim_ready: false,
            valkyrie_ready: false,
            dll_translation,
            dxgi_bridge: DxgiPresentBridge::new(),
            updates: AtomicUpdateManager::new(),
            probes: BTreeMap::new(),
            next_probe_id: AtomicU64::new(1),
        }
    }

    pub fn bootstrap_isolation(&mut self) {
        crate::ironshim_bridge::init_ironshim_bridge();
        self.ironshim_ready = true;
        self.valkyrie_ready = crate::valkyrie_virt::init_valkyrie().is_ok();
    }

    pub fn plan_for(&self, target: SubsystemTarget, risk: RiskTier) -> IsolationPlan {
        match target {
            SubsystemTarget::Win32 => IsolationPlan {
                // Win32 sürücü/köprü tarafında IronShim zorunlu.
                requires_ironshim: true,
                // Yüksek riskli süreç/sürücü etkileşimi Valkyrie domainine alınır.
                requires_valkyrie: matches!(risk, RiskTier::High | RiskTier::Critical),
                domain_label: "win32-compat-domain",
            },
            SubsystemTarget::Els => IsolationPlan {
                // eLS tarafında ABI+driver uyumluluğu için birlikte zorunlu sınır.
                requires_ironshim: true,
                requires_valkyrie: true,
                domain_label: "els-linux-domain",
            },
        }
    }

    pub fn enforce_plan(&self, plan: IsolationPlan) -> Result<(), EcosystemError> {
        if plan.requires_ironshim && !self.ironshim_ready {
            return Err(EcosystemError::IsolationUnavailable);
        }
        if plan.requires_valkyrie && !self.valkyrie_ready {
            return Err(EcosystemError::IsolationUnavailable);
        }
        Ok(())
    }

    pub fn prepare_win32_image(
        &self,
        image: &[u8],
        risk: RiskTier,
    ) -> Result<Win32PreparedImage, EcosystemError> {
        let plan = self.plan_for(SubsystemTarget::Win32, risk);
        self.enforce_plan(plan)?;
        let pe = load_pe(image)?;
        let translated_modules = self
            .dll_translation
            .iter()
            .map(|(k, v)| format!("{}=>{}", k, v))
            .collect::<Vec<_>>();
        let dxvk_targets = self.dxgi_bridge.describe_targets();
        let abi_modules = crate::win32_abi::signed_module_registry()
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        Ok(Win32PreparedImage {
            image: pe,
            isolation: plan,
            translated_modules,
            dxvk_targets,
            abi_modules,
        })
    }

    pub fn prepare_els_image(
        &self,
        image: &[u8],
        risk: RiskTier,
    ) -> Result<ElsPreparedImage, EcosystemError> {
        let plan = self.plan_for(SubsystemTarget::Els, risk);
        self.enforce_plan(plan)?;
        let elf = parse_elf(image)?;
        Ok(ElsPreparedImage {
            image: elf,
            isolation: plan,
        })
    }

    pub fn translate_dll(&self, dll_name: &str) -> Option<String> {
        self.dll_translation
            .get(&dll_name.to_lowercase())
            .cloned()
    }

    pub fn dxvk_target(&self, api: &str) -> Option<String> {
        self.dxgi_bridge
            .target_for(api)
            .map(|backend| backend.as_str().to_string())
    }

    pub fn present_dxgi(&self, request: DxgiPresentRequest) -> Result<DxgiPresentResult, EcosystemError> {
        self.dxgi_bridge.present(request)
    }

    pub fn update_manager(&self) -> &AtomicUpdateManager {
        &self.updates
    }

    pub fn update_manager_mut(&mut self) -> &mut AtomicUpdateManager {
        &mut self.updates
    }

    pub fn validate_bhd_permissions(
        &self,
        manifest: &BhdPermissionManifest,
        requested: &[String],
    ) -> bool {
        if manifest.signature_required && requested.is_empty() {
            return false;
        }
        requested.iter().all(|cap| manifest.capabilities.iter().any(|c| c == cap))
    }

    /// bpftrace uyumlu spec:
    /// - `kprobe:<func>`
    /// - `uprobe:<path>:<symbol>`
    pub fn attach_bpftrace_probe(
        &mut self,
        spec: &str,
        program_id: u32,
    ) -> Result<u64, EcosystemError> {
        let (kind, attach_key) = parse_probe_spec(spec)?;
        ebpf::bpf_prog_attach(program_id, BpfAttachPoint::KernelFunc(attach_key.clone()))?;

        let id = self.next_probe_id.fetch_add(1, Ordering::Relaxed);
        self.probes.insert(
            id,
            RegisteredProbe {
                id,
                kind,
                spec: spec.to_string(),
                program_id,
            },
        );
        Ok(id)
    }

    pub fn fire_kprobe(&self, function_name: &str, ctx_ptr: u64) {
        let addr = bpf_hook_hash(function_name.as_bytes());
        ebpf::kprobe_fire(addr, ctx_ptr);
    }

    pub fn fire_uprobe(&self, binary_path: &str, symbol: &str, ctx_ptr: u64) {
        let key = format!("uprobe:{}:{}", binary_path, symbol);
        let addr = bpf_hook_hash(key.as_bytes());
        ebpf::kprobe_fire(addr, ctx_ptr);
    }

    pub fn collect_dump_artifacts(&self) -> Option<DumpArtifacts> {
        let crash = kdump::last_crash()?;
        Some(DumpArtifacts {
            vmcore: crash.to_vmcore(),
            minidump: crash.to_minidump(),
        })
    }

    pub fn driver_compatibility_matrix(
        &self,
    ) -> Vec<crate::drivers::dispatcher::DriverCompatibilityRow> {
        crate::drivers::dispatcher::compatibility_matrix()
    }

    pub fn anti_cheat_parity_snapshot(
        &self,
    ) -> crate::security::anti_cheat::AntiCheatParitySnapshot {
        crate::security::anti_cheat::snapshot()
    }

    pub fn anti_cheat_attestation(
        &self,
        last_seq: u64,
    ) -> crate::security::anti_cheat::AntiCheatAttestationReport {
        crate::security::anti_cheat::attestation_report(last_seq)
    }
}

impl Default for EcosystemCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_probe_spec(spec: &str) -> Result<(ProbeKind, String), EcosystemError> {
    if let Some(name) = spec.strip_prefix("kprobe:") {
        if name.is_empty() {
            return Err(EcosystemError::InvalidProbeSpec);
        }
        return Ok((ProbeKind::Kprobe, name.to_string()));
    }
    if let Some(rest) = spec.strip_prefix("uprobe:") {
        let mut parts = rest.split(':');
        let path = parts.next().unwrap_or("");
        let symbol = parts.next().unwrap_or("");
        if path.is_empty() || symbol.is_empty() {
            return Err(EcosystemError::InvalidProbeSpec);
        }
        return Ok((ProbeKind::Uprobe, format!("uprobe:{}:{}", path, symbol)));
    }
    Err(EcosystemError::InvalidProbeSpec)
}

fn bpf_hook_hash(data: &[u8]) -> u64 {
    // src/ebpf.rs içindeki simple_hash ile aynı çarpan.
    let mut hash: u64 = 5381;
    for &b in data {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

lazy_static::lazy_static! {
    static ref ECOSYSTEM: Mutex<EcosystemCoordinator> = Mutex::new(EcosystemCoordinator::new());
}

pub fn coordinator() -> &'static Mutex<EcosystemCoordinator> {
    &ECOSYSTEM
}

pub fn bootstrap() {
    let mut guard = coordinator().lock();
    guard.bootstrap_isolation();
}

pub fn prepare_win32(image: &[u8], risk: RiskTier) -> Result<Win32PreparedImage, EcosystemError> {
    coordinator().lock().prepare_win32_image(image, risk)
}

pub fn prepare_els(image: &[u8], risk: RiskTier) -> Result<ElsPreparedImage, EcosystemError> {
    coordinator().lock().prepare_els_image(image, risk)
}

pub fn driver_compatibility_matrix() -> Vec<crate::drivers::dispatcher::DriverCompatibilityRow> {
    coordinator().lock().driver_compatibility_matrix()
}

pub fn anti_cheat_parity_snapshot() -> crate::security::anti_cheat::AntiCheatParitySnapshot {
    coordinator().lock().anti_cheat_parity_snapshot()
}

pub fn present_dxgi(request: DxgiPresentRequest) -> Result<DxgiPresentResult, EcosystemError> {
    coordinator().lock().present_dxgi(request)
}

pub fn anti_cheat_attestation(
    last_seq: u64,
) -> crate::security::anti_cheat::AntiCheatAttestationReport {
    coordinator().lock().anti_cheat_attestation(last_seq)
}
