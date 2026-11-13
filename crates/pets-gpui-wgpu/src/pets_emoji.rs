//! Pets patch: эмодзи — цветным шрифтом.
//!
//! cosmic-text ищет замену глифу по общему списку Unix, где «Noto Color Emoji» — последним, после
//! «DejaVu Sans», «Noto Sans Symbols2» и др. Эти шрифты покрывают часть эмодзи (😀, ❤, ☀) одноцветным контуром:
//! эмодзи выходят чёрными значками или не рисуются вовсе. Указать «Noto Color Emoji» запасным шрифтом в `Font`
//! нельзя: `load_family` отбрасывает начертания без глифа `m`. Поэтому графема с эмодзи-представлением сразу
//! получает свой отрезок с известным цветным шрифтом, если он есть в системе и покрывает её первый символ.

/// Семейства цветных эмодзи в порядке предпочтения.
pub(crate) const EMOJI_FAMILIES: &[&str] = &[
    "Noto Color Emoji",
    "Twemoji",
    "Twitter Color Emoji",
    "JoyPixels",
    "EmojiOne Color",
    "Apple Color Emoji",
    "Segoe UI Emoji",
];

/// Графема показывается как эмодзи (UTS #51: `Emoji_Presentation`, `FE0F`, keycap, флаг, последовательность).
pub(crate) fn is_emoji_grapheme(grapheme: &str) -> bool {
    let Some(first) = grapheme.chars().next() else {
        return false;
    };
    if grapheme.contains('\u{FE0E}') {
        // Явно запрошено текстовое представление.
        return false;
    }
    if grapheme.contains('\u{FE0F}') || grapheme.contains('\u{20E3}') {
        return true;
    }
    let cp = first as u32;
    match cp {
        // Региональные индикаторы (флаги) и эмодзи из «Enclosed Alphanumeric Supplement».
        0x1F1E6..=0x1F1FF | 0x1F18E | 0x1F191..=0x1F19A => true,
        0x1F100..=0x1F1E5 => false,
        // Плоскость эмодзи: маджонг, карты, символы и пиктограммы, смайлы, транспорт, дополнения.
        0x1F000..=0x1FAFF => true,
        _ => is_bmp_emoji_presentation(cp),
    }
}

/// `Emoji_Presentation=Yes` в BMP (Unicode 15.1): без `FE0F` эти символы — эмодзи по умолчанию.
fn is_bmp_emoji_presentation(cp: u32) -> bool {
    matches!(
        cp,
        0x231A..=0x231B
            | 0x23E9..=0x23EC
            | 0x23F0
            | 0x23F3
            | 0x25FD..=0x25FE
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267F
            | 0x2693
            | 0x26A1
            | 0x26AA..=0x26AB
            | 0x26BD..=0x26BE
            | 0x26C4..=0x26C5
            | 0x26CE
            | 0x26D4
            | 0x26EA
            | 0x26F2..=0x26F3
            | 0x26F5
            | 0x26FA
            | 0x26FD
            | 0x2705
            | 0x270A..=0x270B
            | 0x2728
            | 0x274C
            | 0x274E
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27B0
            | 0x27BF
            | 0x2B1B..=0x2B1C
            | 0x2B50
            | 0x2B55
    )
}

#[cfg(test)]
mod tests {
    use super::is_emoji_grapheme;

    #[test]
    fn pets_emoji_presentation() {
        for g in [
            "😀",
            "🎉",
            "👍🏽",
            "❤\u{FE0F}",
            "👨\u{200D}👩\u{200D}👧",
            "🇷🇺",
            "1\u{FE0F}\u{20E3}",
            "⭐",
            "✅",
            "⌚",
        ] {
            assert!(is_emoji_grapheme(g), "{g:?}");
        }
        for g in ["a", "Ж", "❤", "☀", "✓", "©", "😀\u{FE0E}", "🄰", ""] {
            assert!(!is_emoji_grapheme(g), "{g:?}");
        }
    }
}
