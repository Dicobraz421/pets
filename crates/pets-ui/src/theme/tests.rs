use std::fmt::Write as _;

use super::*;

const NO_APP: &[AppRole] = &[];

fn os(dark: bool) -> OsLook {
    OsLook {
        dark,
        ..OsLook::default()
    }
}

fn hc_os() -> OsLook {
    OsLook {
        high_contrast: true,
        hc: Some(HcColors {
            window: Color::rgb(0x20_2020),
            window_text: Color::WHITE,
            highlight: Color::rgb(0x8E_E3F0),
            highlight_text: Color::rgb(0x26_3B50),
            gray_text: Color::rgb(0xA6_A6A6),
            hotlight: Color::rgb(0x75_E9FC),
        }),
        ..OsLook::default()
    }
}

fn theme(id: &str, os: &OsLook) -> Theme {
    resolve(&builtin(id).unwrap(), None, os, FontSizes::default(), NO_APP)
}

/// Все встроенные темы во всех видах ОС, с подписью для сообщений.
fn all_builtins() -> Vec<(String, Theme)> {
    let mut out = Vec::new();
    for id in BUILTIN {
        for (look, o) in [("light", os(false)), ("dark", os(true)), ("os-hc", hc_os())] {
            out.push((format!("{id}/{look}"), theme(id, &o)));
        }
    }
    out
}

fn hex(c: Hsla) -> String {
    Color::from_hsla(c).hex()
}

/// Акценты Windows из «Параметров» (верхний ряд по тонам + серый) — как в тесте Урфина.
const ACCENTS: [u32; 8] = [
    0x00_78D4, 0xFF_B900, 0xF7_630C, 0xE8_1123, 0x88_1798, 0x00_99BC, 0x10_893E, 0x76_7676,
];

#[test]
fn builtins_are_complete() {
    for (what, t) in all_builtins() {
        let spec = t.to_spec("export");
        let table = if t.is_dark() { &spec.dark } else { &spec.light };
        assert_eq!(table.len(), Role::COUNT, "{what}");
        if t.appearance() != Appearance::HighContrast {
            for r in Role::ALL {
                assert_ne!(Color::from_hsla(t.c(*r)).a, 0, "{what}: {} is transparent", r.name());
            }
        }
        for m in Metric::ALL {
            assert!(t.m(*m) > 0.0, "{what}: {}", m.name());
        }
    }
    for r in Role::ALL {
        assert_eq!(Role::from_name(r.name()), Some(*r));
    }
}

#[test]
fn builtin_text_contrast_is_wcag_aa() {
    for (what, t) in all_builtins() {
        for bg in [Role::LayerPanel, Role::LayerOverlay, Role::BgBase] {
            for fg in [Role::TextPrimary, Role::TextSecondary] {
                let k = contrast(t.c(fg), t.c(bg));
                assert!(k >= MIN_CONTRAST, "{what}: {}/{}: {k:.2}", fg.name(), bg.name());
            }
        }
    }
}

#[test]
fn accent_contrast_for_system_accents() {
    for v in ACCENTS {
        let o = OsLook {
            accent: AccentPalette::from_base(Color::rgb(v)),
            ..OsLook::default()
        };
        for dark in [false, true] {
            let t = theme(SYSTEM, &OsLook { dark, ..o });
            let panel = t.c(Role::LayerPanel);
            let checks = [
                (
                    "accent.on/accent.fill",
                    t.c(Role::AccentOn),
                    over(t.c(Role::AccentFill), panel),
                ),
                (
                    "accent.text/accent.subtle",
                    t.c(Role::AccentText),
                    over(t.c(Role::AccentSubtle), panel),
                ),
            ];
            for (what, fg, bg) in checks {
                let k = contrast(fg, bg);
                assert!(k >= MIN_CONTRAST, "#{v:06X} dark={dark} {what}: {k:.2}");
            }
        }
    }
}

/// Те же значения, что у Урфина (снимок `urfin_ui__tokens__tests__token_table_snapshot`, акцент #0078D4 в OKLCH).
#[test]
fn values_mirror_urfin_tokens() {
    let o = OsLook {
        accent: AccentPalette::from_base(Color::rgb(0x00_78D4)),
        ..OsLook::default()
    };
    let light = theme(SYSTEM, &o);
    let dark = theme(SYSTEM, &OsLook { dark: true, ..o });
    let mut got = String::new();
    for r in Role::ALL {
        let _ = writeln!(got, "{:<22} {:<10} {}", r.name(), hex(light.c(*r)), hex(dark.c(*r)));
    }
    for m in Metric::ALL {
        let _ = writeln!(got, "{:<22} {}", m.name(), light.m(*m));
    }
    let want = "\
accent.fill            #0061AD    #5BABFF
accent.fill.hover      #E60061AD  #E65BABFF
accent.fill.pressed    #CC0061AD  #CC5BABFF
accent.text            #004B87    #90C5FF
accent.on              #FFFFFF    #000000
accent.subtle          #1F0078D4  #295BABFF
text.primary           #E4000000  #FFFFFF
text.secondary         #9E000000  #C5FFFFFF
text.tertiary          #72000000  #8BFFFFFF
text.disabled          #5C000000  #5DFFFFFF
bg.base                #F3F3F3    #202020
bg.base.alt            #EEEEEE    #1C1C1C
layer.panel            #FFFFFF    #272727
layer.overlay          #F9F9F9    #2C2C2C
fill.subtle.hover      #09000000  #0FFFFFFF
fill.subtle.pressed    #06000000  #0AFFFFFF
fill.cursor            #0F000000  #15FFFFFF
fill.cursor.passive    #06000000  #0AFFFFFF
fill.control           #B3FFFFFF  #0FFFFFFF
fill.control.input     #FFFFFF    #1E1E1E
stroke.card            #0F000000  #19000000
stroke.control         #0F000000  #12FFFFFF
stroke.control.bottom  #72000000  #8BFFFFFF
stroke.strong          #72000000  #8BFFFFFF
stroke.divider         #14000000  #15FFFFFF
shadow.overlay         #24000000  #42000000
critical.fg            #C42B1C    #FF99A4
critical.bg            #FDE7E9    #442726
caution.fg             #9D5D00    #FCE100
caution.bg             #FFF4CE    #433519
success.fg             #0F7B0F    #6CCB5F
success.bg             #DFF6DD    #393D1B
info.bg                #80F6F6F6  #08FFFFFF
space.xxs              2
space.xs               4
space.s                8
space.m                12
space.l                16
space.xl               20
space.xxl              24
radius.control         4
radius.overlay         8
stroke.thin            1
focus.outer            2
focus.inner            1
h.titlebar             40
h.tab                  32
h.breadcrumb           32
h.col_header           28
h.row                  24
h.panel_footer         28
h.job_row              32
h.banner               48
h.cmdline              32
h.keybar               24
h.control              24
h.menu_item            28
w.icon                 16
w.pill                 3
w.accent_bar           3
gap.panels             8
pad.row_x              12
type.caption           12
type.body              14
type.subtitle          20
type.mono              13
";
    assert_eq!(got, want);
}

#[test]
fn density_scale() {
    let rows: Vec<u16> = Density::ALL.iter().map(|d| d.row()).collect();
    assert_eq!(rows, [20, 24, 28, 32]);
    assert_eq!(Density::default(), Density::Normal);
    for d in Density::ALL {
        assert_eq!(Density::from_row(i64::from(d.row())), Some(d));
        let spec = ThemeSpec {
            density: Some(d),
            ..ThemeSpec::default()
        };
        let t = resolve(&spec, None, &os(false), FontSizes::default(), NO_APP);
        let row = f32::from(d.row());
        assert_eq!(t.m(Metric::HRow), row);
        assert_eq!(t.m(Metric::HControl), row);
        assert_eq!(t.m(Metric::HMenuItem), row + 4.0);
        assert_eq!(t.m(Metric::HTab), row + 8.0);
        let top = if d == Density::Spacious { 24.0 } else { row };
        assert_eq!(
            t.m(Metric::SpaceXxl),
            top,
            "top of the spacing scale is the row height (Spacious: as Normal)"
        );
        assert_eq!(t.m(Metric::HTitlebar), 40.0, "title bar does not depend on density");
        let space = d.spacing();
        assert!(space.windows(2).all(|w| w[0] < w[1]), "{d:?}: {space:?}");
        assert!(space.iter().all(|v| (v * 10.0).fract() == 0.0), "≤ 1 decimal");
    }
    assert_eq!(Density::from_row(30), None);
    let small = Density::Compact.spacing();
    let big = Density::Comfortable.spacing();
    assert!(small.iter().zip(big).all(|(s, b)| *s <= b));
}

#[test]
fn font_sizes_follow_body() {
    let fonts = FontSizes { body: 16.0, mono: 12.0 };
    let t = resolve(&ThemeSpec::default(), None, &os(false), fonts, NO_APP);
    assert_eq!(
        [
            t.m(Metric::TypeCaption),
            t.m(Metric::TypeBody),
            t.m(Metric::TypeSubtitle),
            t.m(Metric::TypeMono)
        ],
        [14.0, 16.0, 22.0, 12.0]
    );
}

const APP: &[AppRole] = &[AppRole {
    name: "app.label.blue",
    light: Color::rgb(0x00_67C0),
    dark: Color::rgb(0x7D_B8E8),
    hc: Role::AccentFill,
}];

const FULL: &str = r##"
name = "Моя \"тема\""
appearance = "dark"
accent = "#E81123"
density = 28

[tokens.light]
"layer.panel" = "#80FFFFFF"

[tokens.dark]
"text.primary" = "#D2D2D2"
"text.secondary" = "#A6A6A6"
"app.label.blue" = "#8CC4F0"
"##;

#[test]
fn toml_round_trip() {
    let spec = ThemeSpec::parse("mine", FULL, APP).unwrap();
    assert_eq!(spec.name.as_deref(), Some("Моя \"тема\""));
    assert_eq!(spec.appearance, Some(Appearance::Dark));
    assert_eq!(spec.accent, Some(Accent::Color(Color::rgb(0xE8_1123))));
    assert_eq!(spec.density, Some(Density::Comfortable));
    assert_eq!(spec.light["layer.panel"], Color::argb(0x80FF_FFFF));
    assert_eq!(spec.dark["app.label.blue"], Color::rgb(0x8C_C4F0));
    let text = spec.to_toml();
    assert_eq!(ThemeSpec::parse("other", &text, APP).unwrap(), spec, "{text}");

    // Итоговая тема → файл → та же тема.
    for (what, t) in all_builtins() {
        let back = ThemeSpec::parse("x", &t.to_spec("export").to_toml(), NO_APP).unwrap();
        assert_eq!(back.id, "export");
        let again = resolve(&back, None, &os(false), FontSizes::default(), NO_APP);
        for r in Role::ALL {
            assert_eq!(hex(again.c(*r)), hex(t.c(*r)), "{what}: {}", r.name());
        }
        assert_eq!(again.density(), t.density());
    }
}

#[test]
fn spec_layers_apply_and_missing_roles_fall_back() {
    let spec = ThemeSpec::parse("mine", FULL, APP).unwrap();
    let t = resolve(&spec, None, &os(false), FontSizes::default(), APP);
    assert_eq!(t.appearance(), Appearance::Dark, "appearance pins the variant");
    assert_eq!(hex(t.c(Role::TextPrimary)), "#D2D2D2");
    assert_eq!(hex(t.app("app.label.blue")), "#8CC4F0");
    let base = theme(FLUENT_DARK, &os(false));
    assert_eq!(
        t.c(Role::LayerPanel),
        base.c(Role::LayerPanel),
        "light table is not used in dark"
    );
    assert_eq!(
        t.c(Role::CriticalFg),
        base.c(Role::CriticalFg),
        "missing role → base theme"
    );
    assert_ne!(t.c(Role::AccentFill), base.c(Role::AccentFill), "own accent");
    assert_eq!(t.m(Metric::HRow), 28.0);

    // Слой пользователя сильнее темы.
    let user = ThemeSpec::parse(
        "user",
        "density = 20\naccent = \"system\"\n[tokens.dark]\n\"text.primary\" = \"#FFFFFF\"",
        APP,
    )
    .unwrap();
    let t = resolve(&spec, Some(&user), &os(false), FontSizes::default(), APP);
    assert_eq!(hex(t.c(Role::TextPrimary)), "#FFFFFF");
    assert_eq!(hex(t.c(Role::TextSecondary)), "#A6A6A6");
    assert_eq!(t.c(Role::AccentFill), base.c(Role::AccentFill));
    assert_eq!(t.density(), Density::Compact);

    // Роли приложения: значения по умолчанию для варианта.
    let light = resolve(&ThemeSpec::default(), None, &os(false), FontSizes::default(), APP);
    assert_eq!(hex(light.app("app.label.blue")), "#0067C0");
    assert_eq!(light.app("app.nope"), light.c(Role::TextPrimary));
}

#[test]
fn nested_tables_are_flattened() {
    let text = "[tokens.dark.text]\nprimary = \"#D2D2D2\"\n[tokens.dark.app.label]\nblue = \"#7DB8E8\"";
    let spec = ThemeSpec::parse("n", text, APP).unwrap();
    assert_eq!(spec.dark.keys().collect::<Vec<_>>(), ["app.label.blue", "text.primary"]);
}

#[test]
fn validation_errors_list_every_problem() {
    let text = r##"
colour = "red"
density = 30
appearance = "sepia"
accent = "blue"
[tokens.light]
"text.primary" = "white"
"no.such" = "#000000"
"type.archive" = "#8764B8"
"app.label.blue" = "#0067C0"
[tokens.paper]
"text.primary" = "#000000"
"##;
    let Err(ThemeError::Invalid { id, issues }) = ThemeSpec::parse("bad", text, NO_APP) else {
        panic!("must be invalid");
    };
    assert_eq!(id, "bad");
    assert_eq!(
        issues,
        [
            Issue::BadValue {
                key: "accent",
                value: "\"blue\"".into()
            },
            Issue::BadValue {
                key: "appearance",
                value: "\"sepia\"".into()
            },
            Issue::UnknownKey("colour".into()),
            Issue::BadValue {
                key: "density",
                value: "30".into()
            },
            Issue::BadColor {
                table: "light",
                role: "text.primary".into(),
                value: "\"white\"".into()
            },
            Issue::UnknownRoles {
                table: "light",
                roles: vec!["app.label.blue".into(), "no.such".into(), "type.archive".into()]
            },
            Issue::UnknownKey("tokens.paper".into()),
        ]
    );
    let msg = ThemeError::Invalid { id, issues }.to_string();
    assert!(
        msg.contains("unknown roles in [tokens.light]: app.label.blue, no.such, type.archive"),
        "{msg}"
    );

    assert!(matches!(
        ThemeSpec::parse("s", "name = ", NO_APP),
        Err(ThemeError::Syntax { .. })
    ));
    assert!(ThemeSpec::parse("e", "", NO_APP).is_ok(), "empty file = base theme");
}

#[test]
fn os_high_contrast_wins_over_everything() {
    let user = ThemeSpec::parse("u", "[tokens.light]\n\"text.primary\" = \"#123456\"", NO_APP).unwrap();
    let spec = ThemeSpec::parse("mine", FULL, APP).unwrap();
    let o = hc_os();
    let hc = o.hc.unwrap();
    let t = resolve(&spec, Some(&user), &o, FontSizes::default(), APP);
    assert_eq!(t.appearance(), Appearance::HighContrast);
    assert!(t.is_dark());
    assert_eq!(
        t.c(Role::TextPrimary),
        hc.window_text.hsla(),
        "user overrides do not apply"
    );
    assert_eq!(t.c(Role::LayerPanel), hc.window.hsla());
    assert_eq!(t.c(Role::AccentFill), hc.highlight.hsla());
    assert_eq!(t.c(Role::AccentOn), hc.highlight_text.hsla());
    assert_eq!(
        t.app("app.label.blue"),
        hc.highlight.hsla(),
        "app role → its HC core role"
    );
    assert_eq!(t.m(Metric::HRow), 28.0, "density still applies");
    for r in Role::ALL {
        let a = Color::from_hsla(t.c(*r)).a;
        assert!(a == 0 || a == 255, "no translucency: {}", r.name());
    }
    // Тема «Высокая контрастность» без режима ОС — классическая чёрная.
    let t = theme(HIGH_CONTRAST, &os(false));
    assert_eq!(t.c(Role::LayerPanel), Color::BLACK.hsla());
    assert_eq!(t.c(Role::TextPrimary), Color::WHITE.hsla());
}

#[test]
fn system_follows_os_and_pinned_themes_do_not() {
    assert_eq!(theme(SYSTEM, &os(false)).appearance(), Appearance::Light);
    assert_eq!(theme(SYSTEM, &os(true)).appearance(), Appearance::Dark);
    assert_eq!(theme(FLUENT_LIGHT, &os(true)).appearance(), Appearance::Light);
    assert_eq!(theme(FLUENT_DARK, &os(false)).appearance(), Appearance::Dark);
    let green = OsLook {
        accent: AccentPalette::from_base(Color::rgb(0x10_7C10)),
        ..os(false)
    };
    assert_ne!(
        theme(SYSTEM, &green).c(Role::AccentFill),
        theme(SYSTEM, &os(false)).c(Role::AccentFill)
    );
}

fn cfg(choice: &str) -> ThemeConfig {
    ThemeConfig {
        choice: choice.into(),
        user_themes: vec![
            ThemeSpec::parse("mine", FULL, APP).unwrap(),
            ThemeSpec {
                id: "fluent-dark".into(),
                ..ThemeSpec::default()
            },
            ThemeSpec {
                id: "Bad Id".into(),
                ..ThemeSpec::default()
            },
        ],
        app_roles: APP,
        ..ThemeConfig::default()
    }
}

#[test]
fn state_choice_and_os_changes() {
    let mut s = ThemeState::new(cfg(SYSTEM), os(false), Policy::default());
    assert_eq!(
        s.ids().collect::<Vec<_>>(),
        [SYSTEM, FLUENT_LIGHT, FLUENT_DARK, HIGH_CONTRAST, "mine"]
    );
    assert_eq!(s.theme().appearance(), Appearance::Light);
    assert!(s.set_os_dark(true));
    assert!(!s.set_os_dark(true), "nothing changed");
    assert_eq!(s.theme().appearance(), Appearance::Dark);
    assert!(s.set_choice("mine").unwrap());
    assert_eq!(hex(s.theme().c(Role::TextPrimary)), "#D2D2D2");
    assert_eq!(s.set_choice("paper"), Err(ThemeError::Unknown("paper".into())));
    assert_eq!(s.choice(), "mine");
    assert!(s.set_os(hc_os()));
    assert_eq!(s.theme().appearance(), Appearance::HighContrast);
    assert!(s.set_os(os(true)));
    assert!(s.set_density(Some(Density::Compact)));
    assert_eq!(s.theme().density(), Density::Compact);
    // Тему из профиля удалили — «как в ОС».
    assert!(s.set_user_themes(Vec::new()));
    assert_eq!(s.effective(), SYSTEM);
    assert_eq!(s.choice(), "mine", "choice is kept for when the file comes back");
}

#[test]
fn policy_locks_choice() {
    let forced = Policy {
        theme: Some(FLUENT_DARK.into()),
        allow_change: None,
    };
    let mut s = ThemeState::new(
        ThemeConfig {
            overrides: Some(
                ThemeSpec::parse(
                    "u",
                    "density = 20\n[tokens.dark]\n\"text.primary\" = \"#123456\"",
                    NO_APP,
                )
                .unwrap(),
            ),
            ..cfg("mine")
        },
        os(false),
        forced,
    );
    assert!(s.locked());
    assert_eq!(s.effective(), FLUENT_DARK);
    assert_eq!(s.theme().appearance(), Appearance::Dark);
    assert_eq!(
        s.theme().c(Role::TextPrimary),
        theme(FLUENT_DARK, &os(false)).c(Role::TextPrimary),
        "no user colors"
    );
    assert_eq!(s.theme().density(), Density::Compact, "density stays the user's");
    assert_eq!(s.set_choice(FLUENT_LIGHT), Err(ThemeError::Locked));

    let no_change = Policy {
        theme: None,
        allow_change: Some(false),
    };
    assert_eq!(no_change.forced(), Some(SYSTEM));
    let s = ThemeState::new(cfg("mine"), os(true), no_change);
    assert_eq!(s.effective(), SYSTEM);
    let allowed = Policy {
        theme: None,
        allow_change: Some(true),
    };
    assert!(!allowed.locked());
    assert_eq!(ThemeState::new(cfg("mine"), os(true), allowed).effective(), "mine");
}

#[test]
fn load_dir_reads_valid_and_reports_bad() {
    let dir = std::env::temp_dir().join(format!("pets-ui-themes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Mine.toml"), FULL).unwrap();
    std::fs::write(dir.join("broken.toml"), "[tokens.dark]\n\"no.such\" = \"#000000\"").unwrap();
    std::fs::write(dir.join("fluent-dark.toml"), "").unwrap();
    std::fs::write(dir.join("notes.txt"), "not a theme").unwrap();
    let (themes, errors) = load_dir(&dir, APP);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(themes.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["mine"]);
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(matches!(&errors[0], ThemeError::Invalid { id, .. } if id == "broken"));
    assert_eq!(errors[1], ThemeError::Reserved("fluent-dark".into()));
    assert_eq!(
        load_dir(&dir, APP),
        (Vec::new(), Vec::new()),
        "missing folder — no themes, no errors"
    );
}

#[test]
fn texts_for_builtin_titles_exist() {
    let ftl = include_str!("../../i18n/ru.ftl");
    for key in [
        "pets-ui-theme-system",
        "pets-ui-theme-fluent-light",
        "pets-ui-theme-fluent-dark",
        "pets-ui-theme-high-contrast",
    ] {
        assert!(ftl.contains(key), "{key}");
    }
}

/// Чтение ОС на стенде: значения разумные, политики у несуществующего приложения нет.
#[test]
fn os_probe_and_policy_read() {
    let look = OsLook::read();
    println!("{look:?}");
    assert_eq!(look.hc.is_some(), look.high_contrast);
    assert_eq!(Policy::read("PetsUiTestNoSuchApp"), Policy::default());
}

/// Тексты интерфейса — только через Fluent: в коде крейта нет литералов с кириллицей.
#[test]
fn no_raw_ui_strings() {
    let sources = [
        ("lib.rs", include_str!("../lib.rs")),
        ("toasts.rs", include_str!("../toasts.rs")),
        ("theme/mod.rs", include_str!("mod.rs")),
        ("theme/state.rs", include_str!("state.rs")),
        ("theme/spec.rs", include_str!("spec.rs")),
        ("theme/policy.rs", include_str!("policy.rs")),
        ("theme/roles.rs", include_str!("roles.rs")),
        ("theme/builtin.rs", include_str!("builtin.rs")),
        ("theme/color.rs", include_str!("color.rs")),
        ("theme/accent.rs", include_str!("accent.rs")),
        ("theme/sys/windows.rs", include_str!("sys/windows.rs")),
        ("theme/sys/other.rs", include_str!("sys/other.rs")),
    ];
    for (file, src) in sources {
        assert_eq!(pets_i18n::raw_strings(src), vec![], "{file}");
    }
}

/// Урфин: «стандарт» 32 px — строка и элементы управления 32, отступы как при 24.
#[test]
fn spacious_is_urfin_standard() {
    assert_eq!(Density::Spacious.row(), 32);
    assert_eq!(Density::Spacious.spacing(), Density::Normal.spacing());
    assert_eq!(Density::from_row(32), Some(Density::Spacious));
    let m = super::roles::metrics(Density::Spacious, Default::default());
    let n = super::roles::metrics(Density::Normal, Default::default());
    let at = |metrics: &[f32], metric: Metric| metrics[Metric::ALL.iter().position(|x| *x == metric).expect("metric")];
    assert_eq!((at(&m, Metric::HRow), at(&m, Metric::HControl)), (32.0, 32.0));
    assert_eq!(at(&m, Metric::SpaceL), at(&n, Metric::SpaceL));
}
