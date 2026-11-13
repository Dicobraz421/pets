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
    /// Спрос без кадра дольше порога при видимом окне. `since` — начало спроса, `waited_ms` — с начала
    /// спроса или с момента, когда окно снова стало видно.
    Began { since: u64, waited_ms: u64 },
    /// Окно не видно (перекрыто, другой Space, свёрнуто), а gpui ждёт кадра дольше порога — не пауза.
    Hidden { since: u64, waited_ms: u64 },
    /// Кадр пришёл: пауза длилась `after_ms`; `resumed_after_ms` — когда до этого ссылку
    /// возобновило пробуждение или запуск (от начала паузы).
    Ended {
        after_ms: u64,
        cause: FrameCause,
        resumed_after_ms: Option<u64>,
    },
    /// Кадр пришёл к окну, которое ждало его скрытым.
    HiddenEnded { after_ms: u64, cause: FrameCause },
}

#[derive(Clone, Copy)]
struct Watch {
    /// Начало спроса.
    demand: u64,
    /// Начало паузы: спрос или последний тик, когда окно было скрыто.
    start: u64,
    hidden: bool,
}

/// Логика порога сторожа для одного окна — чистая функция времени.
#[derive(Default)]
pub(crate) struct StallTracker {
    /// Спрос, по которому уже написано событие.
    watch: Option<Watch>,
    /// Последний тик, когда окно было скрыто.
    hidden_at: Option<u64>,
}

impl StallTracker {
    /// `first_frame` — первый кадр после начала спроса, о котором написано событие.
    pub fn tick(
        &mut self,
        now: u64,
        demand_since: Option<u64>,
        first_frame: Option<(u64, FrameCause)>,
        last_resume: Option<u64>,
        hidden: bool,
        threshold_ms: u64,
    ) -> Option<Stall> {
        let hidden_before = self.hidden_at;
        if hidden {
            self.hidden_at = Some(now);
        }
        let Some(watch) = self.watch else {
            let demand = demand_since?;
            let start = if hidden {
                demand
            } else {
                demand.max(hidden_before.unwrap_or(0))
            };
            let waited_ms = now.saturating_sub(start);
            if waited_ms < threshold_ms {
                return None;
            }
            self.watch = Some(Watch { demand, start, hidden });
            return Some(if hidden {
                Stall::Hidden {
                    since: demand,
                    waited_ms,
                }
            } else {
                Stall::Began {
                    since: demand,
                    waited_ms,
                }
            });
        };
        if let Some((at, cause)) = first_frame.filter(|&(at, _)| at >= watch.demand) {
            self.watch = None;
            return Some(if watch.hidden {
                Stall::HiddenEnded {
                    after_ms: at.saturating_sub(watch.demand),
                    cause,
                }
            } else {
                Stall::Ended {
                    after_ms: at.saturating_sub(watch.start),
                    cause,
                    resumed_after_ms: last_resume
                        .filter(|&resume| resume >= watch.start && resume <= at)
                        .map(|resume| resume - watch.start),
                }
            });
        }
        // Окно снова видно, а кадра всё нет: пауза — от последнего тика, когда оно было скрыто.
        if watch.hidden && !hidden {
            let start = hidden_before.unwrap_or(watch.demand).max(watch.demand);
            let waited_ms = now.saturating_sub(start);
            if waited_ms >= threshold_ms {
                self.watch = Some(Watch {
                    start,
                    hidden: false,
                    ..watch
                });
                return Some(Stall::Began {
                    since: watch.demand,
                    waited_ms,
                });
            }
        }
        None
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
    fn pets_stall_begins_only_after_threshold_and_ends_on_first_frame() {
        let mut tracker = StallTracker::default();
        let t = 500;
        assert_eq!(tracker.tick(100, None, None, None, false, t), None, "no demand");
        assert_eq!(
            tracker.tick(599, Some(100), None, None, false, t),
            None,
            "below threshold"
        );
        assert_eq!(
            tracker.tick(600, Some(100), None, None, false, t),
            Some(Stall::Began {
                since: 100,
                waited_ms: 500
            })
        );
        assert_eq!(tracker.tick(900, Some(100), None, None, false, t), None, "written once");
        let old_frame = Some((50, FrameCause::MetalCallback));
        assert_eq!(
            tracker.tick(950, Some(100), old_frame, None, false, t),
            None,
            "frame before demand"
        );
        assert_eq!(
            tracker.tick(
                8_000,
                None,
                Some((7_700, FrameCause::DisplayLayer)),
                Some(7_600),
                false,
                t
            ),
            Some(Stall::Ended {
                after_ms: 7_600,
                cause: FrameCause::DisplayLayer,
                resumed_after_ms: Some(7_500)
            })
        );
        assert_eq!(
            tracker.tick(8_100, None, None, None, false, t),
            None,
            "idle after the stall"
        );
    }

    #[test]
    fn pets_stall_ignores_resume_outside_the_stall() {
        let mut tracker = StallTracker::default();
        let t = 500;
        tracker.tick(1_000, Some(500), None, None, false, t);
        assert_eq!(
            tracker.tick(
                1_250,
                Some(1_100),
                Some((1_100, FrameCause::CvCallback)),
                Some(10),
                false,
                t
            ),
            Some(Stall::Ended {
                after_ms: 600,
                cause: FrameCause::CvCallback,
                resumed_after_ms: None
            })
        );
        assert_eq!(
            tracker.tick(1_500, Some(1_100), None, None, false, t),
            None,
            "new demand counts from its own start"
        );
        assert_eq!(
            tracker.tick(1_600, Some(1_100), None, None, false, t),
            Some(Stall::Began {
                since: 1_100,
                waited_ms: 500
            })
        );
    }

    #[test]
    fn pets_hidden_window_demand_is_not_a_stall() {
        let mut tracker = StallTracker::default();
        let t = 500;
        assert_eq!(
            tracker.tick(600, Some(0), None, None, true, t),
            Some(Stall::Hidden {
                since: 0,
                waited_ms: 600
            })
        );
        assert_eq!(tracker.tick(5_000, Some(0), None, None, true, t), None, "written once");
        assert_eq!(
            tracker.tick(5_250, None, Some((5_040, FrameCause::MetalCallback)), None, false, t),
            Some(Stall::HiddenEnded {
                after_ms: 5_040,
                cause: FrameCause::MetalCallback
            })
        );
    }

    #[test]
    fn pets_stall_after_window_shows_counts_from_visibility() {
        let mut tracker = StallTracker::default();
        let t = 500;
        tracker.tick(600, Some(0), None, None, true, t);
        tracker.tick(1_000, Some(0), None, None, true, t);
        assert_eq!(
            tracker.tick(1_250, Some(0), None, None, false, t),
            None,
            "shown 250 ms ago"
        );
        assert_eq!(
            tracker.tick(1_500, Some(0), None, None, false, t),
            Some(Stall::Began {
                since: 0,
                waited_ms: 500
            })
        );
        assert_eq!(
            tracker.tick(9_000, None, Some((8_000, FrameCause::DisplayLayer)), None, false, t),
            Some(Stall::Ended {
                after_ms: 7_000,
                cause: FrameCause::DisplayLayer,
                resumed_after_ms: None
            })
        );
    }

    #[test]
    fn pets_demand_right_after_hiding_counts_from_visibility() {
        let mut tracker = StallTracker::default();
        let t = 500;
        tracker.tick(250, None, None, None, true, t);
        tracker.tick(500, None, None, None, true, t);
        assert_eq!(
            tracker.tick(750, Some(400), None, None, false, t),
            None,
            "visible only since t=500"
        );
        assert_eq!(
            tracker.tick(1_000, Some(400), None, None, false, t),
            Some(Stall::Began {
                since: 400,
                waited_ms: 500
            })
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
