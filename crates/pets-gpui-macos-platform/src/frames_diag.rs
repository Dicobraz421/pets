//! Pets patch: диагностика кадров (GMAC-01) — цель `gpui_macos::frames` и сторож пауз.
//!
//! Включается раз за процесс, если `GPUI_FRAMES_WATCHDOG=1` или цель `gpui_macos::frames` открыта
//! на debug. Выключена — ни записей, ни потока сторожа: окна не заводят [`WindowDiag`], а каждая
//! точка записи — проверка `Option`.
//!
//! Запись: `t=<мс от запуска> w<окно>` (`w0` — приложение) и событие. События идут в журнал
//! (DEBUG — смены состояния, TRACE — каждый кадр и пробуждение) и в кольцо последних ~64; кольцо
//! печатает сторож в WARN `frame stall`. Сторож — поток раз в 250 мс: если gpui просит кадр
//! (`schedule_frame` или пробуждение после последнего кадра) дольше 500 мс, а кадра нет, — WARN со
//! снимком; когда кадр пришёл — WARN `frame stall ended after N ms` и чем закончилась пауза.
//! Сторож только пишет в журнал, ничего не чинит.
//!
//! «Окно грязное / есть `on_next_frame`» платформе не видно: gpui сообщает о спросе только
//! вызовами `schedule_frame` и пробуждения (`wake_platform`), и в конце кадра зовёт их ровно
//! тогда, когда окно осталось грязным или ждут колбэки следующего кадра. Это и есть «спрос».

pub(crate) use crate::frames_logic::{FrameCause, Ring, Stall, StallTracker, WindowFlags};
use block2::RcBlock;
use objc2::{msg_send, runtime::AnyObject};
use std::{
    collections::HashMap,
    fmt,
    ptr::NonNull,
    sync::{
        Arc, Mutex, OnceLock, PoisonError, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

pub(crate) const TARGET: &str = "gpui_macos::frames";
/// Сколько ждать кадра при спросе, прежде чем писать `frame stall`.
pub(crate) const STALL_THRESHOLD_MS: u64 = 500;
const WATCHDOG_PERIOD: Duration = Duration::from_millis(250);
const RING_CAPACITY: usize = 64;
const NONE: u64 = u64::MAX;

static START: OnceLock<Instant> = OnceLock::new();
static RING: Mutex<Ring> = Mutex::new(Ring::new(RING_CAPACITY));
static WINDOWS: Mutex<Vec<Weak<WindowDiag>>> = Mutex::new(Vec::new());
static NEXT_WINDOW: AtomicU32 = AtomicU32::new(1);
/// 0 — неизвестно, 1 — активно, 2 — нет.
static APP_ACTIVE: AtomicU8 = AtomicU8::new(0);
static SCREENS_ASLEEP: AtomicBool = AtomicBool::new(false);
static SYSTEM_ASLEEP: AtomicBool = AtomicBool::new(false);
/// `NSAppSleepDisabled` из NSUserDefaults: 0 — не прочитано, 1 — да, 2 — нет.
static APP_SLEEP_DISABLED: AtomicU8 = AtomicU8::new(0);

/// Включена ли диагностика (решается при первом вызове — после настройки журнала).
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let on = std::env::var_os("GPUI_FRAMES_WATCHDOG").is_some_and(|v| v == "1")
            || log::log_enabled!(target: TARGET, log::Level::Debug);
        if on {
            START.get_or_init(Instant::now);
            start_watchdog();
        }
        on
    })
}

/// Миллисекунды от включения диагностики (монотонные).
pub(crate) fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Событие: в кольцо и в журнал (`level`). `window` 0 — приложение.
pub(crate) fn event(level: log::Level, window: u32, args: fmt::Arguments) {
    let at = now_ms();
    let text = args.to_string();
    log::log!(target: TARGET, level, "t={at}ms w{window} {text}");
    RING.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(at, window, text);
}

macro_rules! frames_event {
    ($level:ident, $window:expr, $($arg:tt)+) => {
        $crate::frames_diag::event(log::Level::$level, $window, format_args!($($arg)+))
    };
}
pub(crate) use frames_event;

pub(crate) fn set_app_sleep_disabled(disabled: bool) {
    APP_SLEEP_DISABLED.store(if disabled { 1 } else { 2 }, Relaxed);
}

pub(crate) fn set_app_active(active: bool) {
    APP_ACTIVE.store(if active { 1 } else { 2 }, Relaxed);
}

/// Уведомление приложения (активация, сон, Space): запись и состояние для снимка.
pub(crate) fn app_notification(name: &str) {
    let short = name
        .trim_start_matches("NSApplication")
        .trim_start_matches("NSWorkspace")
        .trim_end_matches("Notification");
    match name {
        "NSApplicationDidBecomeActiveNotification" => set_app_active(true),
        "NSApplicationDidResignActiveNotification" => set_app_active(false),
        "NSWorkspaceScreensDidSleepNotification" => SCREENS_ASLEEP.store(true, Relaxed),
        "NSWorkspaceScreensDidWakeNotification" => SCREENS_ASLEEP.store(false, Relaxed),
        "NSWorkspaceWillSleepNotification" => SYSTEM_ASLEEP.store(true, Relaxed),
        "NSWorkspaceDidWakeNotification" => SYSTEM_ASLEEP.store(false, Relaxed),
        _ => {}
    }
    frames_event!(Debug, 0, "app: {short}");
}

/// Путь кадров окна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FramePath {
    Metal,
    CvDisplayLink,
}

/// Кто попросил кадр.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WakeSource {
    /// `PlatformWindow::schedule_frame`.
    ScheduleFrame,
    /// Waker инвалидатора gpui (`wake_platform`: clean→dirty, `on_next_frame`, отложенный кадр).
    Waker,
}

impl WakeSource {
    fn name(self) -> &'static str {
        match self {
            Self::ScheduleFrame => "schedule_frame",
            Self::Waker => "waker",
        }
    }
}

/// Исход `MetalFrames::wake`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WakeOutcome {
    /// Ссылка стояла на паузе — снята.
    Resumed,
    /// Ссылка и так шла.
    Running,
    /// Пробуждение внутри кадра: пауза решится в `end_frame` (отложено).
    Deferred,
    /// Ссылки нет (отсоединена или не запускалась).
    Detached,
    /// `try_borrow` не удался — ссылку в этот момент заменяют.
    Busy,
}

/// Состояние источника кадров окна (для снимка сторожа).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LinkState {
    Detached = 0,
    Running = 1,
    Paused = 2,
}

impl LinkState {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Running,
            2 => Self::Paused,
            _ => Self::Detached,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }
}

/// Итог кадра (вызова колбэка gpui).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct UpdateSummary {
    /// gpui нарисовал сцену (`PlatformWindow::draw`).
    pub drew: bool,
    /// `schedule_frame` за кадр — окно осталось грязным или ждут `on_next_frame`.
    pub schedule_frames: u32,
    /// Пробуждения инвалидатора за кадр.
    pub wakes: u32,
}

impl UpdateSummary {
    pub fn demand(&self) -> bool {
        self.schedule_frames > 0 || self.wakes > 0
    }
}

/// Диагностика одного окна: пишет главный поток, читает сторож.
pub(crate) struct WindowDiag {
    pub id: u32,
    pub path: FramePath,
    /// С какого момента gpui ждёт кадра (`NONE` — не ждёт).
    demand_since: AtomicU64,
    in_update: AtomicBool,
    update_drew: AtomicBool,
    update_schedule_frames: AtomicU32,
    update_wakes: AtomicU32,
    last_frame: AtomicU64,
    last_cause: AtomicU8,
    frames: AtomicU64,
    /// Последнее снятие паузы или запуск ссылки.
    last_resume: AtomicU64,
    link: AtomicU8,
    busy_wakes: AtomicU64,
    detached_wakes: AtomicU64,
    flags: AtomicU8,
    flags_at: AtomicU64,
    display: AtomicU32,
    screen_fps: AtomicU32,
    /// Колбэки CVDisplayLink на его потоке (по ним видно слияние тиков до `step`).
    pub cv_ticks: Arc<AtomicU64>,
    cv_ticks_seen: AtomicU64,
    /// Drawable от Metal-ссылки: выдано, показано (`presentedHandler`, время > 0), отброшено
    /// (`presentedHandler` с нулевым временем), возвращено неиспользованным.
    drawables_issued: AtomicU64,
    drawables_presented: AtomicU64,
    drawables_discarded: AtomicU64,
    drawables_unused: AtomicU64,
    /// Последний `presentedHandler`: наше время и `presentedTime` (биты f64, с).
    last_presented: AtomicU64,
    last_presented_media: AtomicU64,
    /// Слой: `maximumDrawableCount`, `displaySyncEnabled`, `presentsWithTransaction`,
    /// `allowsNextDrawableTimeout` (биты: 1, 2, 4; 0 — не прочитан).
    layer_max_drawables: AtomicU32,
    layer_flags: AtomicU8,
    /// Последние колбэки Metal-ссылки (кольцо, для частоты перед паузой).
    callbacks: [AtomicU64; CALLBACK_HISTORY],
    callback_next: AtomicU32,
    /// Пауза, о которой написал сторож (начало спроса), и первый кадр после неё.
    stalled_since: AtomicU64,
    stall_end: AtomicU64,
    stall_end_cause: AtomicU8,
}

const CALLBACK_HISTORY: usize = 8;

impl WindowDiag {
    /// `None`, если диагностика выключена.
    pub fn new(path: FramePath) -> Option<Arc<Self>> {
        if !enabled() {
            return None;
        }
        let diag = Arc::new(Self {
            id: NEXT_WINDOW.fetch_add(1, Relaxed),
            path,
            demand_since: AtomicU64::new(NONE),
            in_update: AtomicBool::new(false),
            update_drew: AtomicBool::new(false),
            update_schedule_frames: AtomicU32::new(0),
            update_wakes: AtomicU32::new(0),
            last_frame: AtomicU64::new(NONE),
            last_cause: AtomicU8::new(0),
            frames: AtomicU64::new(0),
            last_resume: AtomicU64::new(NONE),
            link: AtomicU8::new(LinkState::Detached as u8),
            busy_wakes: AtomicU64::new(0),
            detached_wakes: AtomicU64::new(0),
            flags: AtomicU8::new(0),
            flags_at: AtomicU64::new(NONE),
            display: AtomicU32::new(0),
            screen_fps: AtomicU32::new(0),
            cv_ticks: Arc::new(AtomicU64::new(0)),
            cv_ticks_seen: AtomicU64::new(0),
            drawables_issued: AtomicU64::new(0),
            drawables_presented: AtomicU64::new(0),
            drawables_discarded: AtomicU64::new(0),
            drawables_unused: AtomicU64::new(0),
            last_presented: AtomicU64::new(NONE),
            last_presented_media: AtomicU64::new(0),
            layer_max_drawables: AtomicU32::new(0),
            layer_flags: AtomicU8::new(0),
            callbacks: std::array::from_fn(|_| AtomicU64::new(NONE)),
            callback_next: AtomicU32::new(0),
            stalled_since: AtomicU64::new(NONE),
            stall_end: AtomicU64::new(NONE),
            stall_end_cause: AtomicU8::new(0),
        });
        let mut windows = WINDOWS.lock().unwrap_or_else(PoisonError::into_inner);
        windows.retain(|w| w.strong_count() > 0);
        windows.push(Arc::downgrade(&diag));
        Some(diag)
    }

    /// gpui попросил кадр.
    pub fn demand(&self, source: WakeSource) {
        if self.in_update.load(Relaxed) {
            match source {
                WakeSource::ScheduleFrame => self.update_schedule_frames.fetch_add(1, Relaxed),
                WakeSource::Waker => self.update_wakes.fetch_add(1, Relaxed),
            };
            return;
        }
        let now = now_ms();
        if self
            .demand_since
            .compare_exchange(NONE, now, Relaxed, Relaxed)
            .is_ok()
        {
            frames_event!(Trace, self.id, "demand began: {}", source.name());
        }
    }

    /// Пробуждение Metal-ссылки и его исход.
    pub fn wake(&self, source: WakeSource, outcome: WakeOutcome) {
        let in_update = self.in_update.load(Relaxed);
        self.demand(source);
        let source = source.name();
        match outcome {
            WakeOutcome::Resumed => {
                self.last_resume.store(now_ms(), Relaxed);
                self.set_link(LinkState::Running);
                frames_event!(Debug, self.id, "wake({source}): resumed paused link");
            }
            WakeOutcome::Running => {
                frames_event!(Trace, self.id, "wake({source}): link running");
            }
            WakeOutcome::Deferred => {
                frames_event!(
                    Trace,
                    self.id,
                    "wake({source}): in update, deferred to end_frame"
                );
            }
            WakeOutcome::Detached => {
                self.detached_wakes.fetch_add(1, Relaxed);
                frames_event!(
                    Debug,
                    self.id,
                    "wake({source}): no link (detached){}",
                    if in_update { ", in update" } else { "" }
                );
            }
            WakeOutcome::Busy => {
                self.busy_wakes.fetch_add(1, Relaxed);
                frames_event!(
                    Debug,
                    self.id,
                    "wake({source}): link busy (try_borrow failed), dropped"
                );
            }
        }
    }

    pub fn set_link(&self, state: LinkState) {
        self.link.store(state as u8, Relaxed);
        if state == LinkState::Running {
            self.last_resume.store(now_ms(), Relaxed);
        }
    }

    /// Колбэк gpui вызван: кадр пришёл.
    pub fn frame_begin(&self, cause: FrameCause) {
        let now = now_ms();
        self.in_update.store(true, Relaxed);
        self.update_drew.store(false, Relaxed);
        self.update_schedule_frames.store(0, Relaxed);
        self.update_wakes.store(0, Relaxed);
        self.frames.fetch_add(1, Relaxed);
        self.last_cause.store(cause as u8, Relaxed);
        self.last_frame.store(now, Relaxed);
        self.demand_since.store(NONE, Relaxed);
        // Первый кадр после паузы, о которой написал сторож, — её конец.
        if self.stalled_since.swap(NONE, Relaxed) != NONE {
            self.stall_end_cause.store(cause as u8, Relaxed);
            self.stall_end.store(now, Relaxed);
        }
    }

    /// Сторож написал `frame stall` по спросу с `since`; конец паузы отметит первый кадр.
    fn mark_stalled(&self, since: u64) {
        self.stalled_since.store(since, Relaxed);
        // Кадр мог прийти между проверкой сторожа и этой отметкой.
        if let Some((at, cause)) = self.last_frame()
            && at >= since
            && self.stalled_since.swap(NONE, Relaxed) != NONE
        {
            self.stall_end_cause.store(cause as u8, Relaxed);
            self.stall_end.store(at, Relaxed);
        }
    }

    fn stall_end(&self) -> Option<(u64, FrameCause)> {
        let at = self.stall_end.load(Relaxed);
        let cause = FrameCause::from_u8(self.stall_end_cause.load(Relaxed))?;
        (at != NONE).then_some((at, cause))
    }

    /// Колбэк Metal-ссылки: интервал с прошлого, до показа по плану ссылки, drawable в работе.
    pub fn metal_callback(&self, target_presentation: f64) {
        let now = now_ms();
        let slot = self.callback_next.fetch_add(1, Relaxed) as usize;
        let previous =
            self.callbacks[(slot + CALLBACK_HISTORY - 1) % CALLBACK_HISTORY].load(Relaxed);
        self.callbacks[slot % CALLBACK_HISTORY].store(now, Relaxed);
        let interval = if previous == NONE || slot == 0 {
            String::from("-")
        } else {
            now.saturating_sub(previous).to_string()
        };
        frames_event!(
            Trace,
            self.id,
            "metal link callback: interval={interval}ms target_present_in={:.1}ms drawables_in_flight={}",
            (target_presentation - media_time()) * 1000.0,
            self.drawables_in_flight()
        );
    }

    /// Ссылка выдала drawable: считаем и вешаем `presentedHandler`.
    pub fn drawable_issued(self: &Arc<Self>, drawable: &AnyObject) {
        self.drawables_issued.fetch_add(1, Relaxed);
        // SAFETY: `drawable` — CAMetalDrawable (MTLDrawable): `drawableID` и
        // `addPresentedHandler:` (macOS 10.15.4+) есть у протокола; блок копируется Metal.
        let id: usize = unsafe { msg_send![drawable, drawableID] };
        let diag = Arc::clone(self);
        let handler = RcBlock::new(move |presented: NonNull<AnyObject>| {
            // SAFETY: Metal передаёт показанный drawable.
            let time: f64 = unsafe { msg_send![presented.as_ref(), presentedTime] };
            diag.drawable_presented(id, time);
        });
        unsafe {
            let _: () = msg_send![drawable, addPresentedHandler: &*handler];
        }
        frames_event!(
            Trace,
            self.id,
            "drawable #{id} issued: drawables_in_flight={}",
            self.drawables_in_flight()
        );
    }

    /// Кадр не взял выданный drawable — он вернулся слою без показа.
    pub fn drawable_unused(&self) {
        self.drawables_unused.fetch_add(1, Relaxed);
    }

    fn drawable_presented(&self, id: usize, time: f64) {
        if time > 0.0 {
            self.drawables_presented.fetch_add(1, Relaxed);
            self.last_presented.store(now_ms(), Relaxed);
            self.last_presented_media.store(time.to_bits(), Relaxed);
            frames_event!(
                Trace,
                self.id,
                "drawable #{id} presented: {:.1}ms ago drawables_in_flight={}",
                (media_time() - time) * 1000.0,
                self.drawables_in_flight()
            );
        } else {
            self.drawables_discarded.fetch_add(1, Relaxed);
            frames_event!(
                Debug,
                self.id,
                "drawable #{id} presentedHandler with presentedTime=0 (not shown) drawables_in_flight={}",
                self.drawables_in_flight()
            );
        }
    }

    /// Выдано минус показано, отброшено и возвращено неиспользованным.
    fn drawables_in_flight(&self) -> u64 {
        self.drawables_issued.load(Relaxed).saturating_sub(
            self.drawables_presented.load(Relaxed)
                + self.drawables_discarded.load(Relaxed)
                + self.drawables_unused.load(Relaxed),
        )
    }

    fn drawables_summary(&self) -> String {
        let last = self.last_presented.load(Relaxed);
        format!(
            "drawables_in_flight={} (issued={} presented={} presentedTime=0:{} unused={}) last_presentedHandler={}",
            self.drawables_in_flight(),
            self.drawables_issued.load(Relaxed),
            self.drawables_presented.load(Relaxed),
            self.drawables_discarded.load(Relaxed),
            self.drawables_unused.load(Relaxed),
            if last == NONE {
                String::from("none")
            } else {
                format!(
                    "t{last}ms ({}ms ago, presentedTime={:.3}s)",
                    now_ms().saturating_sub(last),
                    f64::from_bits(self.last_presented_media.load(Relaxed))
                )
            }
        )
    }

    /// Слой окна (при запуске ссылки и для снимка).
    pub fn set_layer(
        &self,
        max_drawables: u32,
        display_sync: bool,
        presents_with_transaction: bool,
        allows_timeout: bool,
    ) {
        let flags = 8
            | display_sync as u8
            | (presents_with_transaction as u8) << 1
            | (allows_timeout as u8) << 2;
        let changed = self.layer_max_drawables.swap(max_drawables, Relaxed) != max_drawables
            || self.layer_flags.swap(flags, Relaxed) != flags;
        if changed {
            frames_event!(Debug, self.id, "layer: {}", self.layer_summary());
        }
    }

    fn layer_summary(&self) -> String {
        let flags = self.layer_flags.load(Relaxed);
        if flags == 0 {
            return String::from("not read yet");
        }
        format!(
            "maximumDrawableCount={} displaySyncEnabled={} presentsWithTransaction={} allowsNextDrawableTimeout={}",
            self.layer_max_drawables.load(Relaxed),
            flags & 1,
            flags >> 1 & 1,
            flags >> 2 & 1
        )
    }

    fn callback_intervals(&self) -> String {
        let next = self.callback_next.load(Relaxed) as usize;
        let count = next.min(CALLBACK_HISTORY);
        let times: Vec<u64> = (next - count..next)
            .map(|slot| self.callbacks[slot % CALLBACK_HISTORY].load(Relaxed))
            .collect();
        let intervals: Vec<String> = times
            .windows(2)
            .map(|pair| pair[1].saturating_sub(pair[0]).to_string())
            .collect();
        format!(
            "last metal callbacks: t{}ms, intervals [{}] ms",
            times.last().map_or(String::from("-"), u64::to_string),
            intervals.join(", ")
        )
    }

    /// `PlatformWindow::draw` без drawable от ссылки — через `nextDrawable` слоя (внутри
    /// gpui_apple): сколько занял вызов. nil gpui_apple пишет сам: «failed to retrieve next drawable».
    pub fn next_drawable_draw(&self, took: Duration) {
        let ms = took.as_secs_f64() * 1000.0;
        let level = if ms >= 20.0 {
            log::Level::Debug
        } else {
            log::Level::Trace
        };
        event(
            level,
            self.id,
            format_args!("draw via nextDrawable: {ms:.1}ms (nextDrawable wait + encode)"),
        );
    }

    /// Рисует ли окно в drawable от Metal-ссылки (иначе — `nextDrawable`).
    pub fn draws_from_link(&self) -> bool {
        self.path == FramePath::Metal
            && LinkState::from_u8(self.link.load(Relaxed)) != LinkState::Detached
    }

    /// gpui нарисовал сцену.
    pub fn drew(&self) {
        if self.in_update.load(Relaxed) {
            self.update_drew.store(true, Relaxed);
        } else {
            frames_event!(Debug, self.id, "draw outside a frame callback");
        }
    }

    /// Колбэк gpui вернулся. Остался спрос — ждём следующего кадра с этого момента.
    pub fn frame_end(&self) -> UpdateSummary {
        self.in_update.store(false, Relaxed);
        let summary = UpdateSummary {
            drew: self.update_drew.load(Relaxed),
            schedule_frames: self.update_schedule_frames.load(Relaxed),
            wakes: self.update_wakes.load(Relaxed),
        };
        if summary.demand() {
            let _ = self
                .demand_since
                .compare_exchange(NONE, now_ms(), Relaxed, Relaxed);
        }
        summary
    }

    /// Тики CVDisplayLink с прошлого `step` (больше 1 — тики слились в один кадр).
    pub fn take_cv_ticks(&self) -> u64 {
        let ticks = self.cv_ticks.load(Relaxed);
        ticks.saturating_sub(self.cv_ticks_seen.swap(ticks, Relaxed))
    }

    /// Свежая видимость окна; пишет событие, если она изменилась.
    pub fn set_window(&self, flags: WindowFlags, display: u32, screen_fps: u32) {
        let packed = flags.pack();
        let changed = self.flags.swap(packed, Relaxed) != packed
            || self.display.swap(display, Relaxed) != display
            || self.screen_fps.swap(screen_fps, Relaxed) != screen_fps;
        self.flags_at.store(now_ms(), Relaxed);
        if changed {
            frames_event!(
                Debug,
                self.id,
                "window: {flags} screen={display} maximumFramesPerSecond={screen_fps}"
            );
        }
    }

    fn last_frame(&self) -> Option<(u64, FrameCause)> {
        let at = self.last_frame.load(Relaxed);
        let cause = FrameCause::from_u8(self.last_cause.load(Relaxed))?;
        (at != NONE).then_some((at, cause))
    }

    fn demand_since(&self) -> Option<u64> {
        let since = self.demand_since.load(Relaxed);
        (since != NONE).then_some(since)
    }

    fn last_resume(&self) -> Option<u64> {
        let at = self.last_resume.load(Relaxed);
        (at != NONE).then_some(at)
    }
}

fn start_watchdog() {
    let spawned = std::thread::Builder::new()
        .name("gpui-frames-watchdog".into())
        .spawn(watchdog);
    if let Err(error) = spawned {
        log::warn!(target: TARGET, "frames watchdog not started: {error}");
    }
}

fn watchdog() {
    let mut trackers: HashMap<u32, StallTracker> = HashMap::new();
    // Счётчики drawable на начало паузы: (выдано, показано).
    let mut at_stall: HashMap<u32, (u64, u64)> = HashMap::new();
    let mut power = None;
    loop {
        std::thread::sleep(WATCHDOG_PERIOD);
        let current = power_state();
        if power != Some(current) {
            if let Some((thermal, low_power)) = power {
                frames_event!(
                    Debug,
                    0,
                    "power: thermalState {thermal}→{} isLowPowerModeEnabled {}→{}",
                    current.0,
                    low_power as u8,
                    current.1 as u8
                );
            }
            power = Some(current);
        }
        let windows: Vec<Arc<WindowDiag>> = WINDOWS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        trackers.retain(|id, _| windows.iter().any(|w| w.id == *id));
        for window in windows {
            let now = now_ms();
            let tracker = trackers.entry(window.id).or_default();
            let change = tracker.tick(
                now,
                window.demand_since(),
                window.stall_end(),
                window.last_resume(),
                STALL_THRESHOLD_MS,
            );
            match change {
                Some(Stall::Began { waited_ms }) => {
                    window.mark_stalled(now.saturating_sub(waited_ms));
                    at_stall.insert(
                        window.id,
                        (
                            window.drawables_issued.load(Relaxed),
                            window.drawables_presented.load(Relaxed),
                        ),
                    );
                    log::warn!(target: TARGET, "{}", stall_snapshot(&window, now, waited_ms, current));
                }
                Some(Stall::Ended {
                    after_ms,
                    cause,
                    resumed_after_ms,
                }) => {
                    let resumed = resumed_after_ms
                        .map(|ms| format!("; link resumed/started at +{ms} ms"))
                        .unwrap_or_default();
                    let (issued, presented) = at_stall.remove(&window.id).unwrap_or_default();
                    log::warn!(
                        target: TARGET,
                        "t={now}ms w{} frame stall ended after {after_ms} ms: first frame by {}{resumed}; \
                         during stall: drawables issued={} presented={}; now {}",
                        window.id,
                        cause.name(),
                        window.drawables_issued.load(Relaxed).saturating_sub(issued),
                        window.drawables_presented.load(Relaxed).saturating_sub(presented),
                        window.drawables_summary()
                    );
                    RING.lock().unwrap_or_else(PoisonError::into_inner).push(
                        now,
                        window.id,
                        format!("frame stall ended: {}", cause.name()),
                    );
                }
                None => {}
            }
        }
    }
}

fn stall_snapshot(window: &WindowDiag, now: u64, waited_ms: u64, power: (&str, bool)) -> String {
    use fmt::Write as _;
    let mut s = String::new();
    let link = LinkState::from_u8(window.link.load(Relaxed));
    let _ = write!(
        s,
        "t={now}ms w{} frame stall: gpui waits {waited_ms} ms for a frame; path={} link={} \
         wakes_dropped(busy)={} wakes_without_link={} frames={}",
        window.id,
        match window.path {
            FramePath::Metal => "CAMetalDisplayLink",
            FramePath::CvDisplayLink => "CVDisplayLink",
        },
        link.name(),
        window.busy_wakes.load(Relaxed),
        window.detached_wakes.load(Relaxed),
        window.frames.load(Relaxed),
    );
    match window.last_frame() {
        Some((at, cause)) => {
            let _ = write!(s, " last_frame=t{at}ms ({})", cause.name());
        }
        None => s.push_str(" last_frame=none"),
    }
    let _ = write!(
        s,
        "\n  drawables: {}\n  layer: {}\n  {}",
        window.drawables_summary(),
        window.layer_summary(),
        window.callback_intervals()
    );
    match WindowFlags::unpack(window.flags.load(Relaxed)) {
        Some(flags) => {
            let _ = write!(
                s,
                "\n  window (as of t={}ms): {flags} screen={} maximumFramesPerSecond={}",
                window.flags_at.load(Relaxed),
                window.display.load(Relaxed),
                window.screen_fps.load(Relaxed),
            );
        }
        None => s.push_str("\n  window: not read yet"),
    }
    let _ = write!(
        s,
        "\n  app: active={} screens_asleep={} system_asleep={} thermalState={} isLowPowerModeEnabled={} \
         app_nap=unknown (no public API; NSAppSleepDisabled={})",
        match APP_ACTIVE.load(Relaxed) {
            1 => "1",
            2 => "0",
            _ => "?",
        },
        SCREENS_ASLEEP.load(Relaxed) as u8,
        SYSTEM_ASLEEP.load(Relaxed) as u8,
        power.0,
        power.1 as u8,
        match APP_SLEEP_DISABLED.load(Relaxed) {
            1 => "1",
            2 => "0",
            _ => "?",
        },
    );
    let ring = RING.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = write!(s, "\n  recent events:");
    for entry in ring.entries() {
        let _ = write!(s, "\n    {entry}");
    }
    s
}

/// `CACurrentMediaTime` — часы `presentedTime` и меток Metal-ссылки, с.
pub(crate) fn media_time() -> f64 {
    #[link(name = "QuartzCore", kind = "framework")]
    unsafe extern "C" {
        fn CACurrentMediaTime() -> f64;
    }
    // SAFETY: без аргументов и состояния.
    unsafe { CACurrentMediaTime() }
}

/// `thermalState` и `isLowPowerModeEnabled` (NSProcessInfo потокобезопасен).
fn power_state() -> (&'static str, bool) {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
    objc2::rc::autoreleasepool(|_| {
        let info = NSProcessInfo::processInfo();
        let thermal = match info.thermalState() {
            NSProcessInfoThermalState::Nominal => "nominal",
            NSProcessInfoThermalState::Fair => "fair",
            NSProcessInfoThermalState::Serious => "serious",
            NSProcessInfoThermalState::Critical => "critical",
            _ => "unknown",
        };
        (thermal, info.isLowPowerModeEnabled())
    })
}

/// Строка для INFO при запуске: thermal и low power.
pub(crate) fn power_summary() -> String {
    let (thermal, low_power) = power_state();
    format!(
        "thermalState={thermal} isLowPowerModeEnabled={}",
        low_power as u8
    )
}

/// `hw.model` через sysctl.
pub(crate) fn hw_model() -> String {
    use std::ffi::{CStr, c_char, c_int, c_void};
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }
    let mut buf = [0u8; 256];
    let mut len = buf.len();
    // SAFETY: буфер и его длина корректны; новое значение не задаём.
    let rc = unsafe {
        sysctlbyname(
            c"hw.model".as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return "?".into();
    }
    CStr::from_bytes_until_nul(&buf[..len.min(buf.len())])
        .ok()
        .and_then(|model| model.to_str().ok())
        .unwrap_or("?")
        .to_owned()
}
