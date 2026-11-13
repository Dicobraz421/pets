//! Pets patch: чистая логика диагностики кадров (GMAC-01) — кольцо событий, порог сторожа, флаги окна.
//!
//! Только `std`: тесты идут на любой ОС — файл подключён и в тесты `pets-gpui-macos` (форк вне рабочей области
//! и собирается только на macOS).

use std::{collections::VecDeque, fmt};

/// Откуда пришёл кадр (вызов колбэка gpui).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FrameCause {
    MetalCallback = 1,
    CvCallback = 2,
    DisplayLayer = 3,
    BecomeKey = 4,
}

impl FrameCause {
    pub(crate) fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::MetalCallback,
            2 => Self::CvCallback,
            3 => Self::DisplayLayer,
            4 => Self::BecomeKey,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::MetalCallback => "metal link callback",
            Self::CvCallback => "cvdisplaylink callback",
            Self::DisplayLayer => "displayLayer:",
            Self::BecomeKey => "windowDidBecomeKey: sync frame",
        }
    }
}

/// Видимость окна по AppKit — последнее, что видел главный поток.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WindowFlags {
    pub occlusion_visible: bool,
    pub is_visible: bool,
    pub is_key: bool,
    pub is_miniaturized: bool,
    pub on_active_space: bool,
}

impl WindowFlags {
    pub(crate) fn pack(self) -> u8 {
        self.occlusion_visible as u8
            | (self.is_visible as u8) << 1
            | (self.is_key as u8) << 2
            | (self.is_miniaturized as u8) << 3
            | (self.on_active_space as u8) << 4
            | 1 << 7
    }

    pub(crate) fn unpack(bits: u8) -> Option<Self> {
        (bits & 1 << 7 != 0).then_some(Self {
            occlusion_visible: bits & 1 != 0,
            is_visible: bits & 1 << 1 != 0,
            is_key: bits & 1 << 2 != 0,
            is_miniaturized: bits & 1 << 3 != 0,
            on_active_space: bits & 1 << 4 != 0,
        })
    }
}

impl fmt::Display for WindowFlags {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "occlusion_visible={} isVisible={} isKeyWindow={} isMiniaturized={} isOnActiveSpace={}",
            self.occlusion_visible as u8,
            self.is_visible as u8,
            self.is_key as u8,
            self.is_miniaturized as u8,
            self.on_active_space as u8,
        )
    }
}

/// Последние события. Подряд идущие одинаковые сливаются (`×N`), чтобы поток кадров не вытеснял
/// то, что было перед паузой.
pub(crate) struct Ring {
    entries: VecDeque<RingEntry>,
    capacity: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RingEntry {
    pub at: u64,
    pub last_at: u64,
    pub window: u32,
    pub text: String,
    pub repeat: u32,
}

impl Ring {
    pub const fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity,
        }
    }

    pub fn push(&mut self, at: u64, window: u32, text: String) {
        if let Some(last) = self.entries.back_mut()
            && last.window == window
            && last.text == text
        {
            last.repeat += 1;
            last.last_at = at;
            return;
        }
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(RingEntry {
            at,
            last_at: at,
            window,
            text,
            repeat: 1,
        });
    }

    pub fn entries(&self) -> impl Iterator<Item = &RingEntry> {
        self.entries.iter()
    }
}

impl fmt::Display for RingEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "t={}ms w{} {}", self.at, self.window, self.text)?;
        if self.repeat > 1 {
            write!(f, " ×{} (last t={}ms)", self.repeat, self.last_at)?;
        }
        Ok(())
    }
}

/// Переход сторожа.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stall {
    /// Спрос без кадра дольше порога.
    Began { waited_ms: u64 },
    /// Кадр пришёл: пауза длилась `after_ms`; `resumed_after_ms` — когда до этого ссылку
    /// возобновило пробуждение или запуск (от начала спроса).
    Ended {
        after_ms: u64,
        cause: FrameCause,
        resumed_after_ms: Option<u64>,
    },
}

/// Логика порога сторожа для одного окна — чистая функция времени.
#[derive(Default)]
pub(crate) struct StallTracker {
    /// Начало спроса, по которому уже написан `frame stall`.
    since: Option<u64>,
}

impl StallTracker {
    pub fn tick(
        &mut self,
        now: u64,
        demand_since: Option<u64>,
        last_frame: Option<(u64, FrameCause)>,
        last_resume: Option<u64>,
        threshold_ms: u64,
    ) -> Option<Stall> {
        match self.since {
            None => {
                let since = demand_since?;
                let waited_ms = now.saturating_sub(since);
                (waited_ms >= threshold_ms).then(|| {
                    self.since = Some(since);
                    Stall::Began { waited_ms }
                })
            }
            Some(since) => {
                let (at, cause) = last_frame.filter(|&(at, _)| at >= since)?;
                self.since = None;
                Some(Stall::Ended {
                    after_ms: at - since,
                    cause,
                    resumed_after_ms: last_resume
                        .filter(|&resume| resume >= since && resume <= at)
                        .map(|resume| resume - since),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pets_ring_keeps_last_entries_and_merges_repeats() {
        let mut ring = Ring::new(3);
        ring.push(1, 1, "a".into());
        ring.push(2, 1, "a".into());
        ring.push(3, 2, "a".into());
        ring.push(4, 1, "b".into());
        ring.push(5, 1, "c".into());
        let entries: Vec<_> = ring.entries().cloned().collect();
        assert_eq!(entries.len(), 3, "capacity bounds the ring");
        assert_eq!((entries[0].window, entries[0].text.as_str()), (2, "a"));
        assert_eq!(entries[2].text, "c");

        let mut ring = Ring::new(2);
        ring.push(10, 1, "frame".into());
        ring.push(11, 1, "frame".into());
        ring.push(12, 1, "frame".into());
        let entry = ring.entries().next().unwrap();
        assert_eq!((entry.at, entry.last_at, entry.repeat), (10, 12, 3));
        assert_eq!(entry.to_string(), "t=10ms w1 frame ×3 (last t=12ms)");
    }

    #[test]
    fn pets_stall_begins_only_after_threshold_and_ends_on_frame() {
        let mut tracker = StallTracker::default();
        let t = 500;
        assert_eq!(tracker.tick(100, None, None, None, t), None, "no demand");
        assert_eq!(tracker.tick(599, Some(100), None, None, t), None, "below threshold");
        assert_eq!(
            tracker.tick(600, Some(100), None, None, t),
            Some(Stall::Began { waited_ms: 500 })
        );
        assert_eq!(tracker.tick(900, Some(100), None, None, t), None, "written once");
        let old_frame = Some((50, FrameCause::MetalCallback));
        assert_eq!(
            tracker.tick(950, Some(100), old_frame, None, t),
            None,
            "frame before demand"
        );
        assert_eq!(
            tracker.tick(8_000, None, Some((7_700, FrameCause::DisplayLayer)), Some(7_600), t),
            Some(Stall::Ended {
                after_ms: 7_600,
                cause: FrameCause::DisplayLayer,
                resumed_after_ms: Some(7_500),
            })
        );
        assert_eq!(tracker.tick(8_100, None, None, None, t), None, "idle after the stall");
    }

    #[test]
    fn pets_stall_ignores_resume_outside_the_stall() {
        let mut tracker = StallTracker::default();
        let t = 500;
        tracker.tick(1_000, Some(500), None, None, t);
        assert_eq!(
            tracker.tick(1_250, Some(1_100), Some((1_100, FrameCause::CvCallback)), Some(10), t),
            Some(Stall::Ended {
                after_ms: 600,
                cause: FrameCause::CvCallback,
                resumed_after_ms: None,
            })
        );
        assert_eq!(
            tracker.tick(1_500, Some(1_100), None, None, t),
            None,
            "new demand counts from its own start"
        );
        assert_eq!(
            tracker.tick(1_600, Some(1_100), None, None, t),
            Some(Stall::Began { waited_ms: 500 })
        );
    }

    #[test]
    fn pets_window_flags_round_trip() {
        let flags = WindowFlags {
            occlusion_visible: true,
            is_visible: false,
            is_key: true,
            is_miniaturized: false,
            on_active_space: true,
        };
        assert_eq!(WindowFlags::unpack(flags.pack()), Some(flags));
        assert_eq!(WindowFlags::unpack(0), None, "never read");
    }
}
