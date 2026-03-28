//! # echOS GUI Framework (Week-2)
//!
//! Desktop temel taslari:
//! - `protocol`: servisler arasi ortak desktop protokolu
//! - `surface`: piksel tamponu ve metadata yonetimi
//! - `window_manager`: pencere/frame/z-order yonetimi
//! - `damage`: kirli bolge takibi
//! - `client`: native desktop istemci API'si
//! - `focus`: aktif uygulama odak yonetimi

pub mod client;
pub mod damage;
pub mod focus;
pub mod protocol;
pub mod renderer;
pub mod scene;
pub mod shared_ring;
pub mod surface;
pub mod surface_memory;
pub mod window_manager;

pub mod animation;
pub mod effects;
pub mod font;
pub mod icon_pack;
pub mod input_pipeline;
pub mod launch_pipeline;
pub mod layout;
pub mod login;
pub mod scroll_physics;
pub mod shell;
pub mod text;
pub mod theme;
pub mod widgets;
