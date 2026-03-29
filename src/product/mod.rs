#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductSurface {
    Graphics,
    Gui,
    Init,
    Personalization,
    Shell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductSurfaceDescriptor {
    pub surface: ProductSurface,
    pub root: &'static str,
}

pub const PRODUCT_SURFACE_REGISTRY: &[ProductSurfaceDescriptor] = &[
    ProductSurfaceDescriptor {
        surface: ProductSurface::Graphics,
        root: "gfx",
    },
    ProductSurfaceDescriptor {
        surface: ProductSurface::Gui,
        root: "gui",
    },
    ProductSurfaceDescriptor {
        surface: ProductSurface::Init,
        root: "init",
    },
    ProductSurfaceDescriptor {
        surface: ProductSurface::Personalization,
        root: "personalization",
    },
    ProductSurfaceDescriptor {
        surface: ProductSurface::Shell,
        root: "shell",
    },
];

pub const fn product_surface_root(surface: ProductSurface) -> &'static str {
    match surface {
        ProductSurface::Graphics => "gfx",
        ProductSurface::Gui => "gui",
        ProductSurface::Init => "init",
        ProductSurface::Personalization => "personalization",
        ProductSurface::Shell => "shell",
    }
}

pub use super::gfx;
pub use super::gui;
pub use super::init;
pub use super::personalization;
pub use super::shell;
