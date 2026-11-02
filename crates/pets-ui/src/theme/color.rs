//! Цвет sRGB 8 бит без премультипликации: значения тем в TOML и расчёты контраста. В gpui — [`Hsla`].

use gpui_kit::{Hsla, Rgba};

/// sRGB 8 бит на канал, альфа без премультипликации (`#AARRGGBB` WinUI читается без потерь).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color::rgb(0xFF_FFFF);
    pub const BLACK: Color = Color::rgb(0);
    pub const TRANSPARENT: Color = Color::argb(0);

    /// `0xRRGGBB`, непрозрачный.
    pub const fn rgb(v: u32) -> Color {
        Color::argb(0xFF00_0000 | v)
    }

    /// `0xAARRGGBB` (порядок WinUI).
    pub const fn argb(v: u32) -> Color {
        let [a, r, g, b] = v.to_be_bytes();
        Color { r, g, b, a }
    }

    /// `#RRGGBB` или `#AARRGGBB`.
    pub fn parse(s: &str) -> Option<Color> {
        let hex = s.trim().strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        match hex.len() {
            6 => Some(Color::rgb(v)),
            8 => Some(Color::argb(v)),
            _ => None,
        }
    }

    /// `#RRGGBB` для непрозрачного, иначе `#AARRGGBB`.
    pub fn hex(self) -> String {
        let Color { r, g, b, a } = self;
        if a == 255 {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
        }
    }

    pub const fn opaque(self) -> Color {
        Color { a: 255, ..self }
    }

    /// Тот же цвет с долей непрозрачности `0..=1`.
    pub fn with_alpha(self, alpha: f32) -> Color {
        Color {
            a: unit_to_u8(alpha),
            ..self
        }
    }

    pub fn hsla(self) -> Hsla {
        Rgba {
            r: f32::from(self.r) / 255.0,
            g: f32::from(self.g) / 255.0,
            b: f32::from(self.b) / 255.0,
            a: f32::from(self.a) / 255.0,
        }
        .into()
    }

    pub fn from_hsla(c: Hsla) -> Color {
        let Rgba { r, g, b, a } = c.into();
        Color {
            r: unit_to_u8(r),
            g: unit_to_u8(g),
            b: unit_to_u8(b),
            a: unit_to_u8(a),
        }
    }
}

impl From<Color> for Hsla {
    fn from(c: Color) -> Hsla {
        c.hsla()
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=255"
)]
fn unit_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `fg` поверх непрозрачного `bg` (смешение в sRGB, как у композитора); итог непрозрачный.
pub(crate) fn over_c(fg: Color, bg: Color) -> Color {
    let k = f32::from(fg.a) / 255.0;
    let mix = |f: u8, b: u8| unit_to_u8((f32::from(f) * k + f32::from(b) * (1.0 - k)) / 255.0);
    Color {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
        a: 255,
    }
}

/// Относительная яркость WCAG 2.x.
fn luminance(c: Color) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// Контраст WCAG двух непрозрачных цветов (1…21).
pub(crate) fn contrast_c(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Контраст WCAG (1…21). Полупрозрачный `fg` сначала кладётся на `bg`, `bg` считается непрозрачным.
pub fn contrast(fg: Hsla, bg: Hsla) -> f32 {
    let bg = Color::from_hsla(bg).opaque();
    contrast_c(over_c(Color::from_hsla(fg), bg), bg)
}

/// `fg` поверх непрозрачного `bg` — итоговый непрозрачный цвет.
pub fn over(fg: Hsla, bg: Hsla) -> Hsla {
    over_c(Color::from_hsla(fg), Color::from_hsla(bg).opaque()).hsla()
}

pub(crate) fn lerp(a: Color, b: Color, t: f32) -> Color {
    let m = |x: u8, y: u8| unit_to_u8((f32::from(x) + (f32::from(y) - f32::from(x)) * t) / 255.0);
    Color {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: 255,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        for s in ["#0067C0", "#E4000000", "#00FFFFFF", "#FFFFFF"] {
            assert_eq!(Color::parse(s).unwrap().hex(), s);
        }
        assert_eq!(Color::parse("#FF0067C0"), Color::parse("#0067C0"));
        for bad in ["0067C0", "#0067C", "#GG67C0", "white", "#", "#0067C0F"] {
            assert_eq!(Color::parse(bad), None, "{bad}");
        }
    }

    /// Тема хранит `Hsla`: путь `Color → Hsla → Color` точный для любых 8-битных значений.
    #[test]
    fn hsla_round_trip_is_exact() {
        for r in (0..=255u8).step_by(5) {
            for g in (0..=255u8).step_by(3) {
                for b in (0..=255u8).step_by(17) {
                    for a in [0u8, 0x0F, 0x9E, 0xE4, 255] {
                        let c = Color { r, g, b, a };
                        let back = Color::from_hsla(c.hsla());
                        if a == 0 {
                            assert_eq!(back.a, 0);
                        } else {
                            assert_eq!(back, c);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wcag_reference_values() {
        assert!((contrast_c(Color::WHITE, Color::BLACK) - 21.0).abs() < 1e-3);
        // #767676 на белом — классический порог 4,54:1.
        let k = contrast_c(Color::rgb(0x76_7676), Color::WHITE);
        assert!((k - 4.54).abs() < 0.01, "{k}");
        assert_eq!(over_c(Color::argb(0x8000_0000), Color::WHITE), Color::rgb(0x7F_7F7F));
    }
}
