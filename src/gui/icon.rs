//! The padbridge icon: two designs (D1 "cursor ring" and D2 "key to stick"), each drawn in the
//! colours of every theme. The window / taskbar icon and the logo in the top bar follow the
//! current theme and the design picked in Settings. The `.exe` file icon is fixed (D1, Aurora)
//! and is embedded by `build.rs`.

use crate::theme::{self, Theme};
use eframe::egui;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

const SIZE: u32 = 128;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Design {
    D1,
    D2,
}

impl Design {
    pub const ALL: [Design; 2] = [Design::D1, Design::D2];

    pub fn label(self) -> &'static str {
        match self {
            Design::D1 => "Cursor ring",
            Design::D2 => "Key to stick",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Design::D1 => "d1",
            Design::D2 => "d2",
        }
    }

    pub fn from_key(s: &str) -> Option<Design> {
        match s.trim().to_ascii_lowercase().as_str() {
            "d1" => Some(Design::D1),
            "d2" => Some(Design::D2),
            _ => None,
        }
    }
}

static DESIGN: AtomicU8 = AtomicU8::new(0);
/// The (design, theme) the window icon was last set to; 255 = never set.
static APPLIED: AtomicU8 = AtomicU8::new(255);
/// Cached logo textures, keyed by (design, theme).
static TEXTURES: Mutex<Vec<(u8, egui::TextureHandle)>> = Mutex::new(Vec::new());

pub fn design() -> Design {
    if DESIGN.load(Ordering::Relaxed) == 1 {
        Design::D2
    } else {
        Design::D1
    }
}

pub fn set_design(d: Design) {
    DESIGN.store(if d == Design::D2 { 1 } else { 0 }, Ordering::Relaxed);
}

fn theme_index(t: Theme) -> u8 {
    match t {
        Theme::Aurora => 0,
        Theme::Frost => 1,
        Theme::Clear => 2,
    }
}

fn rgba(d: Design, t: Theme) -> &'static [u8] {
    match (d, t) {
        (Design::D1, Theme::Aurora) => include_bytes!("../../assets/icons/d1-aurora.rgba"),
        (Design::D1, Theme::Frost) => include_bytes!("../../assets/icons/d1-frost.rgba"),
        (Design::D1, Theme::Clear) => include_bytes!("../../assets/icons/d1-clear.rgba"),
        (Design::D2, Theme::Aurora) => include_bytes!("../../assets/icons/d2-aurora.rgba"),
        (Design::D2, Theme::Frost) => include_bytes!("../../assets/icons/d2-frost.rgba"),
        (Design::D2, Theme::Clear) => include_bytes!("../../assets/icons/d2-clear.rgba"),
    }
}

/// Window icon data for a design in a theme.
pub fn icon_data(d: Design, t: Theme) -> egui::IconData {
    egui::IconData {
        rgba: rgba(d, t).to_vec(),
        width: SIZE,
        height: SIZE,
    }
}

/// Keeps the window / taskbar icon in step with the current design and theme. Cheap to call
/// every frame: it only sends a command when something changed.
pub fn sync(ctx: &egui::Context) {
    let key = DESIGN.load(Ordering::Relaxed) * 3 + theme_index(theme::current());
    if APPLIED.swap(key, Ordering::Relaxed) != key {
        let data = icon_data(design(), theme::current());
        ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(data))));
    }
}

/// Halves an RGBA image with a 2x2 box filter (premultiplied, so edges stay clean).
fn half(src: &[u8], size: usize) -> Vec<u8> {
    let n = size / 2;
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((y * 2 + dy) * size + x * 2 + dx) * 4;
                let a = src[i + 3] as u32;
                for c in 0..3 {
                    acc[c] += src[i + c] as u32 * a;
                }
                acc[3] += a;
            }
            let o = (y * n + x) * 4;
            for c in 0..3 {
                out[o + c] = acc[c].checked_div(acc[3]).unwrap_or(0) as u8;
            }
            out[o + 3] = (acc[3] / 4) as u8;
        }
    }
    out
}

/// The logo texture for the top bar (the chosen design in the current theme).
pub fn logo(ctx: &egui::Context) -> egui::TextureHandle {
    texture(ctx, design())
}

/// A 64 px texture of a design in the current theme (used for the logo and Settings previews).
pub fn texture(ctx: &egui::Context, d: Design) -> egui::TextureHandle {
    let t = theme::current();
    let key = (if d == Design::D2 { 1 } else { 0 }) * 3 + theme_index(t);
    let mut cache = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, tex)) = cache.iter().find(|(k, _)| *k == key) {
        return tex.clone();
    }
    let px = half(rgba(d, t), SIZE as usize);
    let n = SIZE as usize / 2;
    let image = egui::ColorImage::from_rgba_unmultiplied([n, n], &px);
    let tex = ctx.load_texture(
        format!("padbridge-icon-{}", key),
        image,
        egui::TextureOptions::LINEAR,
    );
    cache.push((key, tex.clone()));
    tex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_the_right_size() {
        for d in Design::ALL {
            for t in Theme::ALL {
                assert_eq!(rgba(d, t).len(), (SIZE * SIZE * 4) as usize, "{d:?} {t:?}");
            }
        }
    }

    #[test]
    fn keys_round_trip() {
        for d in Design::ALL {
            assert_eq!(Design::from_key(d.key()), Some(d));
        }
        assert_eq!(Design::from_key(" D2\n"), Some(Design::D2));
        assert_eq!(Design::from_key("x"), None);
    }

    #[test]
    fn half_keeps_opaque_colour_and_averages_alpha() {
        // 2x2 block: three opaque red pixels and one transparent one.
        let src = [255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 0, 0];
        let out = half(&src, 2);
        assert_eq!(out, vec![255, 0, 0, 191]);
    }
}
