//! Роли цвета и метрики. Имена и значения — как у токенов Урфина (`crates/ui/src/tokens`): имя роли —
//! ключ в таблицах `[tokens.light]` / `[tokens.dark]` темы и конфига.

macro_rules! roles {
    ($($role:ident = $name:literal,)+) => {
        /// Цветовая роль. Имя — ключ TOML (`"text.primary" = "#…"`).
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum Role { $($role,)+ }

        impl Role {
            pub const ALL: &[Role] = &[$(Role::$role,)+];
            pub const COUNT: usize = Role::ALL.len();

            pub const fn name(self) -> &'static str {
                match self { $(Role::$role => $name,)+ }
            }

            pub fn from_name(name: &str) -> Option<Role> {
                match name { $($name => Some(Role::$role),)+ _ => None }
            }
        }
    };
}

roles! {
    AccentFill = "accent.fill",
    AccentFillHover = "accent.fill.hover",
    AccentFillPressed = "accent.fill.pressed",
    AccentText = "accent.text",
    AccentOn = "accent.on",
    AccentSubtle = "accent.subtle",
    TextPrimary = "text.primary",
    TextSecondary = "text.secondary",
    TextTertiary = "text.tertiary",
    TextDisabled = "text.disabled",
    BgBase = "bg.base",
    BgBaseAlt = "bg.base.alt",
    LayerPanel = "layer.panel",
    LayerOverlay = "layer.overlay",
    FillSubtleHover = "fill.subtle.hover",
    FillSubtlePressed = "fill.subtle.pressed",
    FillCursor = "fill.cursor",
    FillCursorPassive = "fill.cursor.passive",
    FillControl = "fill.control",
    FillControlInput = "fill.control.input",
    StrokeCard = "stroke.card",
    StrokeControl = "stroke.control",
    StrokeControlBottom = "stroke.control.bottom",
    StrokeStrong = "stroke.strong",
    StrokeDivider = "stroke.divider",
    ShadowOverlay = "shadow.overlay",
    CriticalFg = "critical.fg",
    CriticalBg = "critical.bg",
    CautionFg = "caution.fg",
    CautionBg = "caution.bg",
    SuccessFg = "success.fg",
    SuccessBg = "success.bg",
    InfoBg = "info.bg",
}

macro_rules! metrics {
    ($($m:ident = $name:literal,)+) => {
        /// Размер в логических px.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Metric { $($m,)+ }

        impl Metric {
            pub const ALL: &[Metric] = &[$(Metric::$m,)+];
            pub const COUNT: usize = Metric::ALL.len();

            pub const fn name(self) -> &'static str {
                match self { $(Metric::$m => $name,)+ }
            }
        }
    };
}

metrics! {
    SpaceXxs = "space.xxs",
    SpaceXs = "space.xs",
    SpaceS = "space.s",
    SpaceM = "space.m",
    SpaceL = "space.l",
    SpaceXl = "space.xl",
    SpaceXxl = "space.xxl",
    RadiusControl = "radius.control",
    RadiusOverlay = "radius.overlay",
    StrokeThin = "stroke.thin",
    FocusOuter = "focus.outer",
    FocusInner = "focus.inner",
    HTitlebar = "h.titlebar",
    HTab = "h.tab",
    HBreadcrumb = "h.breadcrumb",
    HColHeader = "h.col_header",
    HRow = "h.row",
    HPanelFooter = "h.panel_footer",
    HJobRow = "h.job_row",
    HBanner = "h.banner",
    HCmdline = "h.cmdline",
    HKeybar = "h.keybar",
    HControl = "h.control",
    HMenuItem = "h.menu_item",
    WIcon = "w.icon",
    WPill = "w.pill",
    WAccentBar = "w.accent_bar",
    GapPanels = "gap.panels",
    PadRowX = "pad.row_x",
    TypeCaption = "type.caption",
    TypeBody = "type.body",
    TypeSubtitle = "type.subtitle",
    TypeMono = "type.mono",
}

/// Плотность строк: высота строки списка 20 / 24 / 28 / 32 px, по умолчанию 24 (базис Pets; 32 — «стандарт» Урфина).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum Density {
    /// 20 px.
    Compact,
    /// 24 px.
    #[default]
    Normal,
    /// 28 px.
    Comfortable,
    /// 32 px: строка и элементы управления выше, отступы — как у [`Density::Normal`] (Урфин, S2).
    Spacious,
}

impl Density {
    pub const ALL: [Density; 4] = [
        Density::Compact,
        Density::Normal,
        Density::Comfortable,
        Density::Spacious,
    ];

    /// Высота строки, px — и значение `density` в TOML.
    pub const fn row(self) -> u16 {
        match self {
            Density::Compact => 20,
            Density::Normal => 24,
            Density::Comfortable => 28,
            Density::Spacious => 32,
        }
    }

    pub fn from_row(px: i64) -> Option<Density> {
        Density::ALL.into_iter().find(|d| i64::from(d.row()) == px)
    }

    /// Шкала отступов `space.xxs … space.xxl`: верх шкалы равен высоте строки (у [`Density::Spacious`] — как у обычной).
    pub const fn spacing(self) -> [f32; 7] {
        match self {
            Density::Compact => [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 20.0],
            Density::Normal | Density::Spacious => [2.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0],
            Density::Comfortable => [2.0, 6.0, 10.0, 14.0, 20.0, 24.0, 28.0],
        }
    }
}

/// Размеры шрифтов: интерфейс (`type.body`, от него caption −2 и subtitle +6) и моно.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontSizes {
    pub body: f32,
    pub mono: f32,
}

impl Default for FontSizes {
    fn default() -> Self {
        Self { body: 14.0, mono: 13.0 }
    }
}

/// Метрики для плотности и шрифтов. При [`Density::Normal`] и шрифтах по умолчанию — значения Урфина.
pub(crate) fn metrics(density: Density, fonts: FontSizes) -> [f32; Metric::COUNT] {
    let row = f32::from(density.row());
    let [xxs, xs, s, m, l, xl, xxl] = density.spacing();
    let pad = match density {
        Density::Compact => 8.0,
        Density::Normal | Density::Spacious => 12.0,
        Density::Comfortable => 16.0,
    };
    let body = fonts.body.clamp(8.0, 32.0);
    let mono = fonts.mono.clamp(8.0, 32.0);
    let mut out = [0.0; Metric::COUNT];
    for (i, metric) in Metric::ALL.iter().enumerate() {
        out[i] = match metric {
            Metric::SpaceXxs => xxs,
            Metric::SpaceXs => xs,
            Metric::SpaceS => s,
            Metric::SpaceM => m,
            Metric::SpaceL => l,
            Metric::SpaceXl => xl,
            Metric::SpaceXxl => xxl,
            Metric::RadiusControl => 4.0,
            Metric::RadiusOverlay => 8.0,
            Metric::StrokeThin | Metric::FocusInner => 1.0,
            Metric::FocusOuter => 2.0,
            Metric::HTitlebar => 40.0,
            Metric::HBanner => 48.0,
            Metric::HRow | Metric::HKeybar | Metric::HControl => row,
            Metric::HColHeader | Metric::HPanelFooter | Metric::HMenuItem => row + 4.0,
            Metric::HTab | Metric::HBreadcrumb | Metric::HJobRow | Metric::HCmdline => row + 8.0,
            Metric::WIcon => 16.0,
            Metric::WPill | Metric::WAccentBar => 3.0,
            Metric::GapPanels => 8.0,
            Metric::PadRowX => pad,
            Metric::TypeCaption => body - 2.0,
            Metric::TypeBody => body,
            Metric::TypeSubtitle => body + 6.0,
            Metric::TypeMono => mono,
        };
    }
    out
}
