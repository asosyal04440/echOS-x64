//! # echOS Audio Module
//!
//! Professional audio processing system with real-time capabilities.
//! Features:
//! - Real-time audio stream processing
//! - DSP effects (reverb, delay, EQ, compression)
//! - Audio format conversion and resampling
//! - Multi-channel mixing and routing
//! - Low-latency buffer management
//! - Audio device abstraction layer

#![no_std]

pub mod processing_pipeline;

// Re-export commonly used items
pub use processing_pipeline::*;
