//! # echOS Machine Learning Module
//!
//! Machine learning inference engine with ONNX Runtime support.
//! Features:
//! - ONNX model loading and execution
//! - Tensor operations and memory management
//! - CPU/GPU acceleration
//! - Real-time inference pipeline
//! - Model optimization and quantization

#![no_std]

pub mod onnx_runtime;

// Re-export commonly used items
pub use onnx_runtime::*;
