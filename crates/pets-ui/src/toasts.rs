//! Тосты и история уведомлений.
//!
//! Логика очереди ([`Queue`]) — без окна и таймеров, проверяется тестами; [`Toasts`] — view gpui поверх неё.
//! Таймер скрытия живёт, только пока тост на экране: в простое кадров нет (кадры по запросу).
//! Цвета и размеры — роли текущей темы ([`Theme::global`]).

use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::*;
use pets_i18n::tr;

use crate::theme::{Metric, Role, Theme};

/// Важность уведомления: цвет метки и время на экране.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

impl Level {
    /// Сколько тост виден: ошибку успевают прочитать. Тост с действием — [`ACTION_DURATION`].
    pub fn duration(self) -> Duration {
        match self {
            Level::Error => Duration::from_secs(8),
            Level::Warning => Duration::from_secs(6),
            Level::Info | Level::Success => Duration::from_secs(4),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Level::Info => "ℹ",
            Level::Success => "✓",
            Level::Warning => "⚠",
            Level::Error => "✕",
        }
    }

    /// Роль метки. Предупреждение нейтральное: красный — только ошибки (Postman DESIGN «Действия и состояния»).
    pub fn role(self) -> Role {
        match self {
            Level::Info => Role::AccentFill,
            Level::Success => Role::SuccessFg,
            Level::Warning => Role::TextSecondary,
            Level::Error => Role::CriticalFg,
        }
    }
}

/// Тост с действием виден дольше: успеть нажать «Отменить».
pub const ACTION_DURATION: Duration = Duration::from_secs(15);

/// Обработчик действия тоста; вызывается вне обновления [`Toasts`] (`window.defer`).
pub type ActionFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// Кнопка в тосте («Отменить»): подпись (уже переведённая) и обработчик. Срабатывает не больше одного раза.
#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    pub run: ActionFn,
}

impl ToastAction {
    pub fn new(label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            label: label.into(),
            run: Rc::new(run),
        }
    }
}

impl fmt::Debug for ToastAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToastAction")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl PartialEq for ToastAction {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label && Rc::ptr_eq(&self.run, &other.run)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub level: Level,
    pub text: SharedString,
    pub at: Instant,
    /// Действие, пока не выполнено и тост на экране.
    pub action: Option<ToastAction>,
}

impl Toast {
    /// Сколько тост виден.
    pub fn duration(&self) -> Duration {
        if self.action.is_some() {
            ACTION_DURATION
        } else {
            self.level.duration()
        }
    }
}

/// Тосты на экране (не больше [`Queue::MAX_SHOWN`], старый уходит) и история (новые сверху).
#[derive(Debug, Default)]
pub struct Queue {
    shown: Vec<Toast>,
    history: VecDeque<Toast>,
    unread: usize,
    next: u64,
}

impl Queue {
    pub const MAX_SHOWN: usize = 3;
    pub const MAX_HISTORY: usize = 100;

    /// Новый тост; возвращает его id для скрытия по таймеру.
    pub fn push(&mut self, level: Level, text: SharedString, at: Instant) -> u64 {
        self.push_with(level, text, None, at)
    }

    /// Новый тост с необязательным действием.
    pub fn push_with(&mut self, level: Level, text: SharedString, action: Option<ToastAction>, at: Instant) -> u64 {
        self.next += 1;
        let toast = Toast {
            id: self.next,
            level,
            text,
            at,
            action,
        };
        // В истории действия нет: отмена — пока тост на экране.
        self.history.push_front(Toast {
            action: None,
            ..toast.clone()
        });
        self.history.truncate(Self::MAX_HISTORY);
        self.unread = (self.unread + 1).min(Self::MAX_HISTORY);
        self.shown.push(toast);
        if self.shown.len() > Self::MAX_SHOWN {
            self.shown.remove(0);
        }
        self.next
    }

    /// Убрать тост с экрана (в истории остаётся). `false` — его уже нет.
    pub fn hide(&mut self, id: u64) -> bool {
        let before = self.shown.len();
        self.shown.retain(|t| t.id != id);
        self.shown.len() != before
    }

    /// Нажато действие: тост уходит с экрана, действие отдаётся один раз (второе нажатие — `None`).
    pub fn take_action(&mut self, id: u64) -> Option<ToastAction> {
        let i = self.shown.iter().position(|t| t.id == id)?;
        self.shown.remove(i).action
    }

    pub fn shown(&self) -> &[Toast] {
        &self.shown
    }

    /// История, новые сверху.
    pub fn history(&self) -> impl Iterator<Item = &Toast> {
        self.history.iter()
    }

    /// Уведомлений с последнего открытия истории.
    pub fn unread(&self) -> usize {
        self.unread
    }

    /// История просмотрена: тосты на экране больше не нужны.
    pub fn mark_read(&mut self) {
        self.unread = 0;
        self.shown.clear();
    }
}

/// Тосты окна. Встраивается последним ребёнком корня с `.relative()`: рисуется поверх, фокус не берёт.
#[derive(Default)]
pub struct Toasts {
    queue: Queue,
    history_open: bool,
}

impl Toasts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Показать уведомление; скроется само через [`Level::duration`].
    pub fn push(&mut self, level: Level, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_with(level, text, None, cx);
    }

    /// Уведомление с кнопкой (`ToastAction::new(tr!("…-undo"), |window, cx| …)`); видно [`ACTION_DURATION`].
    pub fn push_action(
        &mut self,
        level: Level,
        text: impl Into<SharedString>,
        action: ToastAction,
        cx: &mut Context<Self>,
    ) {
        self.push_with(level, text, Some(action), cx);
    }

    fn push_with(
        &mut self,
        level: Level,
        text: impl Into<SharedString>,
        action: Option<ToastAction>,
        cx: &mut Context<Self>,
    ) {
        let id = self.queue.push_with(level, text.into(), action, Instant::now());
        let duration = self.queue.shown().last().map_or(level.duration(), Toast::duration);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            let _ = this.update(cx, |t, cx| {
                if t.queue.hide(id) {
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Открыть или закрыть историю уведомлений.
    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.history_open = !self.history_open;
        if self.history_open {
            self.queue.mark_read();
        }
        cx.notify();
    }

    pub fn is_history_open(&self) -> bool {
        self.history_open
    }

    /// Для значка в строке состояния.
    pub fn unread(&self) -> usize {
        self.queue.unread()
    }

    pub fn queue(&self) -> &Queue {
        &self.queue
    }
}

const WIDTH: f32 = 360.;

fn shadow(t: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: t.c(Role::ShadowOverlay),
        offset: point(px(0.), px(8.)),
        blur_radius: px(16.),
        spread_radius: px(0.),
        inset: false,
    }]
}

impl Render for Toasts {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::global(cx).clone();
        if self.history_open {
            return history(&self.queue, &theme, cx).into_any_element();
        }
        if self.queue.shown().is_empty() {
            return div().into_any_element();
        }
        let t = &theme;
        div()
            .absolute()
            .top(px(t.m(Metric::HTitlebar) + t.m(Metric::SpaceS)))
            .right(t.px(Metric::SpaceM))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .gap(t.px(Metric::SpaceS))
            .children(self.queue.shown().iter().rev().map(|toast| {
                let id = toast.id;
                let mut c = card(toast, t)
                    .id(("toast", id as usize))
                    .occlude()
                    .cursor_pointer()
                    .shadow(shadow(t))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.queue.hide(id) {
                            cx.notify();
                        }
                    }));
                if let Some(action) = &toast.action {
                    c = c.child(
                        div()
                            .id(("toast-action", id as usize))
                            .flex_none()
                            .px(t.px(Metric::SpaceS))
                            .rounded(t.px(Metric::RadiusControl))
                            .text_color(t.c(Role::AccentText))
                            .font_weight(FontWeight::SEMIBOLD)
                            .hover(|d| d.bg(t.c(Role::FillSubtleHover)))
                            .active(|d| d.bg(t.c(Role::FillSubtlePressed)))
                            .child(action.label.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                if let Some(action) = this.queue.take_action(id) {
                                    cx.notify();
                                    // Обработчик может трогать эти же тосты: не внутри их обновления.
                                    let run = action.run;
                                    window.defer(cx, move |window, cx| run(window, cx));
                                }
                            })),
                    );
                }
                c
            }))
            .into_any_element()
    }
}

fn card(toast: &Toast, t: &Theme) -> Div {
    let mark = t.c(toast.level.role());
    // Цвет у gpui — один на все стороны рамки: метка важности — полоса слева.
    div()
        .relative()
        .overflow_hidden()
        .flex()
        .items_start()
        .gap(t.px(Metric::SpaceS))
        .p(t.px(Metric::SpaceM))
        .pl(t.px(Metric::SpaceL))
        .rounded(t.px(Metric::RadiusOverlay))
        .bg(t.c(Role::LayerOverlay))
        .border_1()
        .border_color(t.c(Role::StrokeCard))
        .text_size(t.px(Metric::TypeBody))
        .text_color(t.c(Role::TextPrimary))
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(t.px(Metric::WAccentBar))
                .bg(mark),
        )
        .child(div().text_color(mark).child(toast.level.icon()))
        .child(div().flex_1().min_w_0().child(toast.text.clone()))
}

fn history(queue: &Queue, t: &Theme, cx: &mut Context<Toasts>) -> impl IntoElement {
    let now = Instant::now();
    let header = div()
        .flex()
        .items_center()
        .px(t.px(Metric::SpaceM))
        .py(t.px(Metric::SpaceS))
        .border_b_1()
        .border_color(t.c(Role::StrokeDivider))
        .child(
            div()
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .child(tr!("pets-ui-notifications")),
        )
        .child(
            div()
                .id("toasts-close")
                .px(t.px(Metric::SpaceS))
                .rounded(t.px(Metric::RadiusControl))
                .cursor_pointer()
                .hover(|d| d.bg(t.c(Role::FillSubtleHover)))
                .child("✕")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_history(cx))),
        );
    let mut list = div()
        .id("toasts-history")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .p(t.px(Metric::SpaceS))
        .gap(t.px(Metric::SpaceS))
        .overflow_y_scroll();
    let mut empty = true;
    for toast in queue.history() {
        empty = false;
        let min = now.duration_since(toast.at).as_secs() / 60;
        list = list.child(
            card(toast, t).child(
                div()
                    .flex_none()
                    .text_size(t.px(Metric::TypeCaption))
                    .text_color(t.c(Role::TextSecondary))
                    .child(tr!("pets-ui-ago", min = min as i64)),
            ),
        );
    }
    if empty {
        list = list.child(
            div()
                .p(t.px(Metric::SpaceS))
                .text_color(t.c(Role::TextSecondary))
                .child(tr!("pets-ui-notifications-empty")),
        );
    }
    div()
        .id("toasts-panel")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .occlude()
        .bg(t.c(Role::LayerOverlay))
        .text_size(t.px(Metric::TypeBody))
        .text_color(t.c(Role::TextPrimary))
        .border_l_1()
        .border_color(t.c(Role::StrokeDivider))
        .shadow(shadow(t))
        .child(header)
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::{ACTION_DURATION, Level, Queue, ToastAction};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Instant;

    #[test]
    fn at_most_three_are_shown_and_all_go_to_history() {
        let mut q = Queue::default();
        let now = Instant::now();
        let ids: Vec<u64> = (0..5)
            .map(|i| q.push(Level::Info, format!("t{i}").into(), now))
            .collect();
        assert_eq!(q.shown().iter().map(|t| t.id).collect::<Vec<_>>(), ids[2..]);
        assert_eq!(
            q.history().map(|t| t.id).collect::<Vec<_>>(),
            ids.iter().rev().copied().collect::<Vec<_>>()
        );
        assert_eq!(q.unread(), 5);
    }

    #[test]
    fn hide_removes_from_screen_only() {
        let mut q = Queue::default();
        let id = q.push(Level::Error, "e".into(), Instant::now());
        assert!(q.hide(id));
        assert!(!q.hide(id));
        assert!(q.shown().is_empty());
        assert_eq!(q.history().count(), 1);
    }

    #[test]
    fn opening_history_marks_read() {
        let mut q = Queue::default();
        q.push(Level::Warning, "w".into(), Instant::now());
        q.mark_read();
        assert_eq!(q.unread(), 0);
        assert!(q.shown().is_empty());
        assert_eq!(q.history().count(), 1);
    }

    #[test]
    fn history_is_capped() {
        let mut q = Queue::default();
        for _ in 0..Queue::MAX_HISTORY + 10 {
            q.push(Level::Info, "x".into(), Instant::now());
        }
        assert_eq!(q.history().count(), Queue::MAX_HISTORY);
    }

    #[test]
    fn errors_stay_longer() {
        assert!(Level::Error.duration() > Level::Info.duration());
    }

    /// «Отменить»: действие одно на тост, срабатывает один раз, в истории его нет, тост держится дольше.
    #[test]
    fn action_is_taken_once_and_not_kept_in_history() {
        let runs = Rc::new(Cell::new(0));
        let r = runs.clone();
        let action = ToastAction::new("Undo", move |_, _| r.set(r.get() + 1));
        let mut q = Queue::default();
        let plain = q.push(Level::Info, "plain".into(), Instant::now());
        let id = q.push_with(
            Level::Info,
            "Moved to Archive".into(),
            Some(action.clone()),
            Instant::now(),
        );
        let shown = q.shown().iter().find(|t| t.id == id).unwrap();
        assert_eq!(shown.action.as_ref(), Some(&action));
        assert_eq!(shown.duration(), ACTION_DURATION);
        assert!(q.history().all(|t| t.action.is_none()));
        assert_eq!(q.take_action(plain), None, "no action — nothing to run");
        assert_eq!(q.shown().len(), 1, "taking an absent action still closes the toast");
        let taken = q.take_action(id).unwrap();
        assert_eq!(taken.label.as_ref(), "Undo");
        assert_eq!(q.take_action(id), None, "second click does nothing");
        assert!(q.shown().is_empty());
        assert_eq!(q.history().count(), 2);
        assert_eq!(runs.get(), 0, "the queue never runs the action itself");
    }

    #[test]
    fn hidden_toast_loses_its_action() {
        let mut q = Queue::default();
        let id = q.push_with(
            Level::Info,
            "x".into(),
            Some(ToastAction::new("Undo", |_, _| {})),
            Instant::now(),
        );
        assert!(q.hide(id));
        assert_eq!(q.take_action(id), None);
    }

    #[test]
    fn texts_exist_in_both_languages() {
        assert_eq!(pets_i18n::check(&[super::super::FTL]), vec![]);
    }
}
