//! Image decode and resize helpers for shell-facing GUI surfaces.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::prelude::*;
use qoi::decode_to_vec;
use tinybmp::Bmp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgbImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl ArgbImage {
    pub fn decode_path(path: &str, bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err(String::from("image decode failed: empty file"));
        }

        if bytes.starts_with(b"qoif") || has_extension(path, "qoi") {
            return Self::decode_qoi(bytes);
        }

        if bytes.starts_with(b"BM") || has_extension(path, "bmp") {
            return Self::decode_bmp(bytes);
        }

        Err(format!(
            "image decode failed: unsupported format for {}",
            path
        ))
    }

    pub fn resize_exact(&self, target_width: u32, target_height: u32) -> Result<Self, String> {
        if self.width == 0 || self.height == 0 {
            return Err(String::from("image resize failed: empty source"));
        }
        let target_width = target_width.max(1);
        let target_height = target_height.max(1);
        if target_width == self.width && target_height == self.height {
            return Ok(self.clone());
        }

        let src_w = self.width as u64;
        let src_h = self.height as u64;
        let dst_w = target_width as u64;
        let dst_h = target_height as u64;
        let src_max_x = src_w.saturating_sub(1);
        let src_max_y = src_h.saturating_sub(1);
        let mut pixels = Vec::with_capacity(target_width as usize * target_height as usize);

        for y in 0..target_height {
            let src_y_fp = if target_height == 1 {
                0
            } else {
                (y as u64 * src_max_y * 65_536) / dst_h.saturating_sub(1)
            };
            let y0 = (src_y_fp >> 16).min(src_max_y) as usize;
            let y1 = (y0 + 1).min(self.height.saturating_sub(1) as usize);
            let fy = (src_y_fp & 0xFFFF) as u32;
            for x in 0..target_width {
                let src_x_fp = if target_width == 1 {
                    0
                } else {
                    (x as u64 * src_max_x * 65_536) / dst_w.saturating_sub(1)
                };
                let x0 = (src_x_fp >> 16).min(src_max_x) as usize;
                let x1 = (x0 + 1).min(self.width.saturating_sub(1) as usize);
                let fx = (src_x_fp & 0xFFFF) as u32;
                let p00 = self.pixels[y0 * self.width as usize + x0];
                let p10 = self.pixels[y0 * self.width as usize + x1];
                let p01 = self.pixels[y1 * self.width as usize + x0];
                let p11 = self.pixels[y1 * self.width as usize + x1];
                pixels.push(bilinear_argb(p00, p10, p01, p11, fx, fy));
            }
        }

        Ok(Self {
            width: target_width,
            height: target_height,
            pixels,
        })
    }

    fn decode_qoi(bytes: &[u8]) -> Result<Self, String> {
        let (header, decoded) =
            decode_to_vec(bytes).map_err(|err| format!("qoi decode failed: {:?}", err))?;
        Self::from_rgba_guess(header.width, header.height, decoded)
    }

    fn decode_bmp(bytes: &[u8]) -> Result<Self, String> {
        let bmp = Bmp::<Rgb888>::from_slice(bytes)
            .map_err(|err| format!("bmp decode failed: {:?}", err))?;
        let size = bmp.size();
        if size.width == 0 || size.height == 0 {
            return Err(String::from("bmp decode failed: empty image"));
        }

        let width = size.width;
        let height = size.height;
        let mut pixels = vec![0xFF000000; width as usize * height as usize];
        for pixel in bmp.pixels() {
            let point = pixel.0;
            if point.x < 0 || point.y < 0 {
                continue;
            }
            let x = point.x as u32;
            let y = point.y as u32;
            if x >= width || y >= height {
                continue;
            }
            let color = pixel.1;
            let index = y as usize * width as usize + x as usize;
            pixels[index] = 0xFF00_0000
                | ((color.r() as u32) << 16)
                | ((color.g() as u32) << 8)
                | color.b() as u32;
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    fn from_rgba_guess(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let pixel_count = width as usize * height as usize;
        if pixel_count == 0 {
            return Err(String::from("image decode failed: zero-sized image"));
        }
        match rgba.len() / pixel_count {
            4 => Self::from_rgba(width, height, rgba),
            3 => {
                let mut expanded = Vec::with_capacity(pixel_count * 4);
                for chunk in rgba.chunks_exact(3) {
                    expanded.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 0xFF]);
                }
                Self::from_rgba(width, height, expanded)
            }
            channels => Err(format!(
                "image decode failed: unsupported channel count {}",
                channels
            )),
        }
    }

    fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let pixel_count = width as usize * height as usize;
        if rgba.len() != pixel_count * 4 {
            return Err(format!(
                "image decode failed: rgba payload {} does not match {} pixels",
                rgba.len(),
                pixel_count
            ));
        }
        let mut pixels = Vec::with_capacity(pixel_count);
        for chunk in rgba.chunks_exact(4) {
            pixels.push(
                ((chunk[3] as u32) << 24)
                    | ((chunk[0] as u32) << 16)
                    | ((chunk[1] as u32) << 8)
                    | chunk[2] as u32,
            );
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }
}

fn bilinear_argb(p00: u32, p10: u32, p01: u32, p11: u32, fx: u32, fy: u32) -> u32 {
    let w0 = (65_536u64.saturating_sub(fx as u64)) * (65_536u64.saturating_sub(fy as u64));
    let w1 = fx as u64 * (65_536u64.saturating_sub(fy as u64));
    let w2 = (65_536u64.saturating_sub(fx as u64)) * fy as u64;
    let w3 = fx as u64 * fy as u64;
    let blend = |shift: u32| -> u32 {
        let c0 = ((p00 >> shift) & 0xFF) as u64;
        let c1 = ((p10 >> shift) & 0xFF) as u64;
        let c2 = ((p01 >> shift) & 0xFF) as u64;
        let c3 = ((p11 >> shift) & 0xFF) as u64;
        (((c0 * w0 + c1 * w1 + c2 * w2 + c3 * w3) + (1 << 31)) >> 32) as u32
    };
    let a = blend(24);
    let r = blend(16);
    let g = blend(8);
    let b = blend(0);
    (a << 24) | (r << 16) | (g << 8) | b
}

fn has_extension(path: &str, wanted: &str) -> bool {
    path.rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case(wanted))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::ArgbImage;
    use alloc::vec;
    use qoi::encode_to_vec;

    #[test]
    fn qoi_decode_roundtrips_rgba_pixels() {
        let pixels = vec![
            0x00, 0x11, 0x22, 0xFF, 0x33, 0x44, 0x55, 0xEE, 0x66, 0x77, 0x88, 0xDD, 0x99, 0xAA,
            0xBB, 0xCC,
        ];
        let encoded = encode_to_vec(&pixels, 2, 2).expect("encode qoi");
        let image = ArgbImage::decode_path("/wallpaper.qoi", &encoded).expect("decode qoi");
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 2);
        assert_eq!(image.pixels[0], 0xFF001122);
        assert_eq!(image.pixels[3], 0xCC99AABB);
    }

    #[test]
    fn bmp_decode_reads_single_pixel() {
        let bmp: [u8; 58] = [
            0x42, 0x4D, 58, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0,
            1, 0, 24, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0x33, 0x22, 0x11, 0x00,
        ];
        let image = ArgbImage::decode_path("/thumb.bmp", &bmp).expect("decode bmp");
        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.pixels, vec![0xFF112233]);
    }

    #[test]
    fn resize_exact_changes_output_dimensions() {
        let image = ArgbImage {
            width: 2,
            height: 2,
            pixels: vec![0xFF102030, 0xFF203040, 0xFF304050, 0xFF405060],
        };
        let resized = image.resize_exact(6, 4).expect("resize");
        assert_eq!(resized.width, 6);
        assert_eq!(resized.height, 4);
        assert_eq!(resized.pixels.len(), 24);
    }
}
