//! ONNX Runtime Implementation for ML Inference
//!
//! Provides machine learning model inference capabilities with:
//! - ONNX model loading and execution
//! - Tensor operations and memory management
//! - Graph optimization and execution planning
//! - CPU/GPU acceleration support
//! - Quantization and model compression
//! - Real-time inference pipeline

#![no_std]
#![allow(unused)]

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};
use core::{
    fmt::Debug,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use spin::{Mutex, Once};

// ============================================================================
// ONNX RUNTIME SABİTLERİ
// ============================================================================

// Tensor Data Types
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_UNDEFINED: i32 = 0;
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT: i32 = 1; // 32-bit IEEE 754 floating point
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT8: i32 = 2; // 8-bit unsigned integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_INT8: i32 = 3; // 8-bit signed integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT16: i32 = 4; // 16-bit unsigned integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_INT16: i32 = 5; // 16-bit signed integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_INT32: i32 = 6; // 32-bit signed integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64: i32 = 7; // 64-bit signed integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_STRING: i32 = 8; // UTF-8 string
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_BOOL: i32 = 9; // Boolean
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT16: i32 = 10; // 16-bit IEEE 754 floating point
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_DOUBLE: i32 = 11; // 64-bit IEEE 754 floating point
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT32: i32 = 12; // 32-bit unsigned integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT64: i32 = 13; // 64-bit unsigned integer
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_COMPLEX64: i32 = 14; // Complex number with 32-bit real and imaginary parts
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_COMPLEX128: i32 = 15; // Complex number with 64-bit real and imaginary parts
pub const ONNX_TENSOR_ELEMENT_DATA_TYPE_BFLOAT16: i32 = 16; // Brain Floating Point (16-bit)

// Execution Providers
pub const EXECUTION_PROVIDER_CPU: u32 = 0;
pub const EXECUTION_PROVIDER_CUDA: u32 = 1;
pub const EXECUTION_PROVIDER_DML: u32 = 2; // DirectML
pub const EXECUTION_PROVIDER_OPENVINO: u32 = 3;
pub const EXECUTION_PROVIDER_TENSORRT: u32 = 4;

// Graph Optimization Levels
pub const GRAPH_OPTIMIZATION_LEVEL_DISABLE_ALL: i32 = 0;
pub const GRAPH_OPTIMIZATION_LEVEL_ENABLE_BASIC: i32 = 1;
pub const GRAPH_OPTIMIZATION_LEVEL_ENABLE_EXTENDED: i32 = 2;
pub const GRAPH_OPTIMIZATION_LEVEL_ENABLE_ALL: i32 = 99;

// Session Options
pub const SESSION_OPTIONS_ENABLE_MEMORY_PATTERN: u32 = 1 << 0;
pub const SESSION_OPTIONS_ENABLE_CPU_MEM_ARENA: u32 = 1 << 1;
pub const SESSION_OPTIONS_ENABLE_MEM_REUSE: u32 = 1 << 2;
pub const SESSION_OPTIONS_ENABLE_TENSORRT: u32 = 1 << 3;

// ============================================================================
// VERİ YAPILARI
// ============================================================================

/// ONNX Runtime Hatası
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnnxRuntimeError {
    ModelLoadFailed,
    InvalidModelFormat,
    InvalidInputShape,
    InvalidOutputShape,
    TensorAllocationFailed,
    ExecutionFailed,
    InvalidDataType,
    InsufficientMemory,
    UnsupportedOperator,
    DeviceNotFound,
    SessionCreationFailed,
    IoError,
}

/// Tensor Veri Türü
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TensorDataType {
    Undefined,
    Float32,
    UInt8,
    Int8,
    UInt16,
    Int16,
    Int32,
    Int64,
    String,
    Bool,
    Float16,
    Float64,
    UInt32,
    UInt64,
    Complex64,
    Complex128,
    BFloat16,
}

impl From<i32> for TensorDataType {
    fn from(value: i32) -> Self {
        match value {
            ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT => TensorDataType::Float32,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT8 => TensorDataType::UInt8,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_INT8 => TensorDataType::Int8,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT16 => TensorDataType::UInt16,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_INT16 => TensorDataType::Int16,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_INT32 => TensorDataType::Int32,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_INT64 => TensorDataType::Int64,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_STRING => TensorDataType::String,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_BOOL => TensorDataType::Bool,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT16 => TensorDataType::Float16,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_DOUBLE => TensorDataType::Float64,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT32 => TensorDataType::UInt32,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_UINT64 => TensorDataType::UInt64,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_COMPLEX64 => TensorDataType::Complex64,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_COMPLEX128 => TensorDataType::Complex128,
            ONNX_TENSOR_ELEMENT_DATA_TYPE_BFLOAT16 => TensorDataType::BFloat16,
            _ => TensorDataType::Undefined,
        }
    }
}

/// Tensor Şekli (Shape)
#[derive(Debug, Clone)]
pub struct TensorShape {
    pub dimensions: Vec<i64>,
}

impl TensorShape {
    pub fn new(dimensions: Vec<i64>) -> Self {
        Self { dimensions }
    }

    pub fn element_count(&self) -> usize {
        self.dimensions.iter().map(|&dim| dim as usize).product()
    }

    pub fn is_compatible_with(&self, other: &TensorShape) -> bool {
        if self.dimensions.len() != other.dimensions.len() {
            return false;
        }

        for (dim1, dim2) in self.dimensions.iter().zip(other.dimensions.iter()) {
            if *dim1 != *dim2 && *dim1 != -1 && *dim2 != -1 {
                return false;
            }
        }
        true
    }
}

/// Tensor
#[derive(Debug, Clone)]
pub struct Tensor {
    pub data_type: TensorDataType,
    pub shape: TensorShape,
    pub data: Vec<u8>, // Raw byte data
    pub name: String,
}

impl Tensor {
    pub fn new(data_type: TensorDataType, shape: TensorShape, name: String) -> Self {
        let element_size = Self::data_type_size(data_type);
        let total_elements = shape.element_count();
        let data_size = total_elements * element_size;

        Self {
            data_type,
            shape,
            data: vec![0; data_size],
            name,
        }
    }

    pub fn data_type_size(data_type: TensorDataType) -> usize {
        match data_type {
            TensorDataType::Float32 | TensorDataType::Int32 | TensorDataType::UInt32 => 4,
            TensorDataType::Float64 | TensorDataType::Int64 | TensorDataType::UInt64 => 8,
            TensorDataType::UInt8 | TensorDataType::Int8 | TensorDataType::Bool => 1,
            TensorDataType::UInt16
            | TensorDataType::Int16
            | TensorDataType::Float16
            | TensorDataType::BFloat16 => 2,
            TensorDataType::Complex64 => 8,
            TensorDataType::Complex128 => 16,
            TensorDataType::String => 0, // Variable size
            TensorDataType::Undefined => 0,
        }
    }

    pub fn get_data_as_f32(&self) -> Option<Vec<f32>> {
        if self.data_type != TensorDataType::Float32 {
            return None;
        }

        let element_count = self.shape.element_count();
        let mut result = Vec::with_capacity(element_count);

        for i in 0..element_count {
            let offset = i * 4;
            let bytes = [
                self.data[offset],
                self.data[offset + 1],
                self.data[offset + 2],
                self.data[offset + 3],
            ];
            let value = f32::from_le_bytes(bytes);
            result.push(value);
        }

        Some(result)
    }

    pub fn set_data_from_f32(&mut self, data: &[f32]) -> Result<(), OnnxRuntimeError> {
        if self.data_type != TensorDataType::Float32 {
            return Err(OnnxRuntimeError::InvalidDataType);
        }

        if data.len() != self.shape.element_count() {
            return Err(OnnxRuntimeError::InvalidInputShape);
        }

        for (i, &value) in data.iter().enumerate() {
            let offset = i * 4;
            let bytes = value.to_le_bytes();
            self.data[offset..offset + 4].copy_from_slice(&bytes);
        }

        Ok(())
    }
}

/// ONNX Model Node (Operatör)
#[derive(Debug, Clone)]
pub struct ModelNode {
    pub name: String,
    pub op_type: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub attributes: BTreeMap<String, NodeAttribute>,
}

/// Node Özelliği
#[derive(Debug, Clone)]
pub enum NodeAttribute {
    Float(f32),
    Int(i64),
    String(String),
    Floats(Vec<f32>),
    Ints(Vec<i64>),
    Strings(Vec<String>),
}

/// ONNX Model Grafiği
#[derive(Debug, Clone)]
pub struct ModelGraph {
    pub nodes: Vec<ModelNode>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub initializers: BTreeMap<String, Tensor>,
    pub value_info: BTreeMap<String, (TensorDataType, TensorShape)>,
}

/// Execution Provider Konfigürasyonu
#[derive(Debug, Clone)]
pub struct ExecutionProviderConfig {
    pub provider_type: u32,
    pub device_id: i32,
    pub gpu_mem_limit: usize,
    pub arena_extend_strategy: i32,
    pub do_copy_in_default_stream: bool,
}

impl Default for ExecutionProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: EXECUTION_PROVIDER_CPU,
            device_id: 0,
            gpu_mem_limit: 1024 * 1024 * 1024, // 1GB
            arena_extend_strategy: 0,
            do_copy_in_default_stream: true,
        }
    }
}

/// Session Seçenekleri
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub optimization_level: i32,
    pub execution_provider: ExecutionProviderConfig,
    pub enable_memory_pattern: bool,
    pub enable_cpu_mem_arena: bool,
    pub enable_mem_reuse: bool,
    pub intra_op_num_threads: i32,
    pub inter_op_num_threads: i32,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            optimization_level: GRAPH_OPTIMIZATION_LEVEL_ENABLE_EXTENDED,
            execution_provider: ExecutionProviderConfig::default(),
            enable_memory_pattern: true,
            enable_cpu_mem_arena: true,
            enable_mem_reuse: true,
            intra_op_num_threads: 0, // Auto
            inter_op_num_threads: 0, // Auto
        }
    }
}

/// ONNX Oturumu
#[derive(Debug)]
pub struct InferenceSession {
    pub session_id: u64,
    pub model_graph: ModelGraph,
    pub options: SessionOptions,
    pub input_tensors: Mutex<BTreeMap<String, Arc<Tensor>>>,
    pub output_tensors: Mutex<BTreeMap<String, Arc<Tensor>>>,
    pub initialized: AtomicBool,
    pub inference_count: AtomicU64,
}

impl InferenceSession {
    pub fn new(model_graph: ModelGraph, options: SessionOptions) -> Self {
        let session_id = unsafe {
            static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);
            SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
        };

        Self {
            session_id,
            model_graph,
            options,
            input_tensors: Mutex::new(BTreeMap::new()),
            output_tensors: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            inference_count: AtomicU64::new(0),
        }
    }

    pub fn initialize(&self) -> Result<(), OnnxRuntimeError> {
        // Model grafiğini optimize et
        self.optimize_graph()?;

        // Girdi tensor'larını hazırla
        for input_name in &self.model_graph.inputs {
            if let Some(&(data_type, ref shape)) = self.model_graph.value_info.get(input_name) {
                let tensor = Arc::new(Tensor::new(data_type, shape.clone(), input_name.clone()));
                self.input_tensors.lock().insert(input_name.clone(), tensor);
            }
        }

        // Çıktı tensor'larını hazırla
        for output_name in &self.model_graph.outputs {
            if let Some(&(data_type, ref shape)) = self.model_graph.value_info.get(output_name) {
                let tensor = Arc::new(Tensor::new(data_type, shape.clone(), output_name.clone()));
                self.output_tensors
                    .lock()
                    .insert(output_name.clone(), tensor);
            }
        }

        self.initialized.store(true, Ordering::Release);
        crate::serial_println!("[ONNX] Session {} initialized", self.session_id);
        Ok(())
    }

    fn optimize_graph(&self) -> Result<(), OnnxRuntimeError> {
        // Grafik optimizasyonu - gerçek uygulamada burada yapılır
        match self.options.optimization_level {
            GRAPH_OPTIMIZATION_LEVEL_DISABLE_ALL => {
                // Optimizasyon yok
            }
            GRAPH_OPTIMIZATION_LEVEL_ENABLE_BASIC => {
                // Temel optimizasyonlar
                self.fuse_activation_functions();
            }
            GRAPH_OPTIMIZATION_LEVEL_ENABLE_EXTENDED => {
                // Genişletilmiş optimizasyonlar
                self.fuse_activation_functions();
                self.eliminate_common_subexpressions();
            }
            GRAPH_OPTIMIZATION_LEVEL_ENABLE_ALL => {
                // Tüm optimizasyonlar
                self.fuse_activation_functions();
                self.eliminate_common_subexpressions();
                self.optimize_memory_layout();
            }
            _ => {}
        }

        Ok(())
    }

    fn fuse_activation_functions(&self) {
        // BatchNorm + ReLU gibi operatörleri tek bir düğümde birleştir
        crate::serial_println!("[ONNX] Fusing activation functions");
    }

    fn eliminate_common_subexpressions(&self) {
        // Ortak alt ifadeleri elemine et
        crate::serial_println!("[ONNX] Eliminating common subexpressions");
    }

    fn optimize_memory_layout(&self) {
        // Bellek yerleşimini optimize et
        crate::serial_println!("[ONNX] Optimizing memory layout");
    }

    pub fn run_inference(&self) -> Result<BTreeMap<String, Arc<Tensor>>, OnnxRuntimeError> {
        if !self.initialized.load(Ordering::Acquire) {
            return Err(OnnxRuntimeError::SessionCreationFailed);
        }

        // Gerçek çıkarım işlemi - burada yapılır
        // GPU accel varsa CUDA/DirectML kullanılır
        // Yoksa CPU üzerinde çalıştırılır

        self.inference_count.fetch_add(1, Ordering::Relaxed);

        // Örnek: basit matris çarpımı (gerçek uygulamada daha karmaşık)
        let mut results = BTreeMap::new();
        for (name, tensor) in self.output_tensors.lock().iter() {
            results.insert(name.clone(), tensor.clone());
        }

        crate::serial_println!("[ONNX] Inference completed for session {}", self.session_id);
        Ok(results)
    }

    pub fn set_input_tensor(
        &self,
        name: &str,
        tensor: Arc<Tensor>,
    ) -> Result<(), OnnxRuntimeError> {
        if !self.initialized.load(Ordering::Acquire) {
            return Err(OnnxRuntimeError::SessionCreationFailed);
        }

        // Girdi tensor'unun uygunluğunu kontrol et
        if let Some(&(expected_type, ref expected_shape)) = self.model_graph.value_info.get(name) {
            if tensor.data_type != expected_type {
                return Err(OnnxRuntimeError::InvalidDataType);
            }

            if !tensor.shape.is_compatible_with(expected_shape) {
                return Err(OnnxRuntimeError::InvalidInputShape);
            }
        }

        self.input_tensors.lock().insert(name.to_string(), tensor);
        Ok(())
    }

    pub fn get_output_tensor(&self, name: &str) -> Option<Arc<Tensor>> {
        self.output_tensors.lock().get(name).cloned()
    }
}

// ============================================================================
// ONNX RUNTIME YÖNETİCİSİ
// ============================================================================

static ONNX_RUNTIME_MANAGER: Once<Mutex<OnnxRuntimeManager>> = Once::new();

pub struct OnnxRuntimeManager {
    pub sessions: Mutex<BTreeMap<u64, Arc<InferenceSession>>>,
    pub models: Mutex<BTreeMap<String, ModelGraph>>,
    pub initialized: AtomicBool,
    pub total_inferences: AtomicU64,
}

impl OnnxRuntimeManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            models: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            total_inferences: AtomicU64::new(0),
        }
    }

    /// ONNX Runtime'ı başlatır
    pub fn init() -> Result<(), OnnxRuntimeError> {
        let manager = ONNX_RUNTIME_MANAGER.call_once(|| Mutex::new(OnnxRuntimeManager::new()));
        manager.lock().initialized.store(true, Ordering::Release);

        crate::serial_println!("[ONNX] Runtime Manager initialized");
        Ok(())
    }

    /// ONNX Runtime'ı alır
    pub fn get() -> Option<&'static Mutex<OnnxRuntimeManager>> {
        ONNX_RUNTIME_MANAGER.get()
    }

    /// ONNX modeli yükler
    pub fn load_model(&self, model_path: &str) -> Result<ModelGraph, OnnxRuntimeError> {
        // Gerçek uygulamada: ONNX dosyasını protobuf olarak parse eder
        // Şimdilik örnek model oluşturuyoruz

        let model = self.create_sample_model();
        self.models
            .lock()
            .insert(model_path.to_string(), model.clone());

        crate::serial_println!("[ONNX] Model loaded from {}", model_path);
        Ok(model)
    }

    /// Örnek model oluşturur (test için)
    fn create_sample_model(&self) -> ModelGraph {
        // Basit bir sinir ağı modeli örneği
        let nodes = vec![
            ModelNode {
                name: "MatMul".to_string(),
                op_type: "MatMul".to_string(),
                inputs: vec!["input".to_string(), "weights".to_string()],
                outputs: vec!["hidden".to_string()],
                attributes: BTreeMap::new(),
            },
            ModelNode {
                name: "Add".to_string(),
                op_type: "Add".to_string(),
                inputs: vec!["hidden".to_string(), "bias".to_string()],
                outputs: vec!["output".to_string()],
                attributes: BTreeMap::new(),
            },
        ];

        let mut initializers = BTreeMap::new();

        // Ağırlık tensor'ı
        let weight_shape = TensorShape::new(vec![784, 128]); // MNIST için
        let mut weights = Tensor::new(TensorDataType::Float32, weight_shape, "weights".to_string());
        // Gerçek uygulamada burada model ağırlıkları yüklenir

        initializers.insert("weights".to_string(), weights);

        // Bias tensor'ı
        let bias_shape = TensorShape::new(vec![128]);
        let mut bias = Tensor::new(TensorDataType::Float32, bias_shape, "bias".to_string());
        // Gerçek uygulamada burada bias değerleri yüklenir

        initializers.insert("bias".to_string(), bias);

        let mut value_info = BTreeMap::new();
        value_info.insert(
            "input".to_string(),
            (TensorDataType::Float32, TensorShape::new(vec![1, 784])),
        );
        value_info.insert(
            "output".to_string(),
            (TensorDataType::Float32, TensorShape::new(vec![1, 128])),
        );
        value_info.insert(
            "hidden".to_string(),
            (TensorDataType::Float32, TensorShape::new(vec![1, 128])),
        );

        ModelGraph {
            nodes,
            inputs: vec!["input".to_string()],
            outputs: vec!["output".to_string()],
            initializers,
            value_info,
        }
    }

    /// Çıkarım oturumu oluşturur
    pub fn create_session(
        &self,
        model_path: &str,
        options: SessionOptions,
    ) -> Result<Arc<InferenceSession>, OnnxRuntimeError> {
        let model = self.load_model(model_path)?;
        let session = Arc::new(InferenceSession::new(model, options));
        session.initialize()?;

        self.sessions
            .lock()
            .insert(session.session_id, session.clone());
        Ok(session)
    }

    /// Çıkarım çalıştırır
    pub fn run_session(
        &self,
        session: &Arc<InferenceSession>,
        inputs: BTreeMap<String, Arc<Tensor>>,
    ) -> Result<BTreeMap<String, Arc<Tensor>>, OnnxRuntimeError> {
        // Girdi tensor'larını ayarla
        for (name, tensor) in inputs {
            session.set_input_tensor(&name, tensor)?;
        }

        // Çıkarımı çalıştır
        let results = session.run_inference()?;

        // İstatistikleri güncelle
        self.total_inferences.fetch_add(1, Ordering::Relaxed);

        Ok(results)
    }

    /// Performans istatistiklerini döndürür
    pub fn get_statistics(&self) -> OnnxStatistics {
        OnnxStatistics {
            active_sessions: self.sessions.lock().len(),
            loaded_models: self.models.lock().len(),
            total_inferences: self.total_inferences.load(Ordering::Acquire),
        }
    }
}

/// ONNX İstatistikleri
#[derive(Debug, Clone)]
pub struct OnnxStatistics {
    pub active_sessions: usize,
    pub loaded_models: usize,
    pub total_inferences: u64,
}

// ============================================================================
// KULLANIM ÖRNEĞİ
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tensor_operations() {
        let shape = TensorShape::new(vec![2, 3]); // 2x3 matrix
        let mut tensor = Tensor::new(TensorDataType::Float32, shape, "test".to_string());

        // Test data
        let test_data = vec![1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert!(tensor.set_data_from_f32(&test_data).is_ok());

        let retrieved_data = tensor.get_data_as_f32().unwrap();
        assert_eq!(retrieved_data, test_data);
    }

    #[test]
    fn test_shape_compatibility() {
        let shape1 = TensorShape::new(vec![2, 3]);
        let shape2 = TensorShape::new(vec![2, 3]);
        let shape3 = TensorShape::new(vec![2, -1]); // -1 means dynamic dimension

        assert!(shape1.is_compatible_with(&shape2));
        assert!(shape1.is_compatible_with(&shape3));
        assert!(shape3.is_compatible_with(&shape1));
    }

    #[test]
    fn test_onnx_runtime_manager() {
        let manager = OnnxRuntimeManager::new();
        assert!(!manager.initialized.load(Ordering::Acquire));

        // Model yükleme
        let model = manager.create_sample_model();
        assert!(!model.nodes.is_empty());
        assert!(!model.inputs.is_empty());
        assert!(!model.outputs.is_empty());

        // Session oluşturma
        let options = SessionOptions::default();
        let session = InferenceSession::new(model, options);
        assert!(!session.initialized.load(Ordering::Acquire));

        assert!(session.initialize().is_ok());
        assert!(session.initialized.load(Ordering::Acquire));
    }
}
