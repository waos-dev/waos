//! The window a program's view request is shown in, in Waft's terms: every option the program
//! set on its `window-config`, under the same name, macOS's title bar among Waft's options for
//! macOS; the size Waft gives a window where the program left it to the platform; the program's
//! name on a window it left untitled, so the user can tell its window from another's; its icon,
//! where the system shows a window's own; on Windows and Linux its identity, the app the taskbar or
//! the dock takes the window for, and on Linux the activation token of the launch it opens for.

use waos::WindowConfig;
use waos::bindings::wasdk::platform::windows::{WindowBackground, WindowLevel};

use crate::link::{ProgramFace, ProgramIdentity};
use crate::logic::title_bar::waft_title_bar;

/// The window `config` asks for, for the program `face` shows, taking the focus with
/// `activation_token` where a launch brought one.
pub(crate) fn window_config_of(
    config: &WindowConfig,
    face: &ProgramFace,
    activation_token: Option<String>,
) -> waft::WindowConfig {
    let platform = waft::WindowConfig::default();
    let title = if config.title.is_empty() {
        face.name.clone()
    } else {
        config.title.clone()
    };
    waft::WindowConfig {
        title,
        size: config
            .size
            .map_or(platform.size, |size| [size.width, size.height]),
        position: config.position.map(|point| [point.x, point.y]),
        decorations: config.decorations,
        resizable: config.resizable,
        level: level_of(config.level),
        activating: config.activating,
        all_desktops: config.all_desktops,
        background: background_of(config.background),
        shadow: config.shadow,
        visible: config.visible,
        icon: face.icon.clone(),
        macos: waft::MacosWindowConfig {
            title_bar: waft_title_bar(config.macos_title_bar),
        },
        windows: windows_config_of(face.identity.as_ref()),
        linux: waft::LinuxWindowConfig {
            app_id: face
                .identity
                .as_ref()
                .map(|identity| identity.app_id.clone()),
            activation_token,
        },
    }
}

/// What a window of the program `identity` names is on Windows: that app on the taskbar, with what
/// its pin starts; the process's where it has none.
pub(crate) fn windows_config_of(identity: Option<&ProgramIdentity>) -> waft::WindowsWindowConfig {
    match identity {
        Some(identity) => waft::WindowsWindowConfig {
            app_user_model_id: Some(identity.app_id.clone()),
            relaunch: identity.relaunch.clone(),
        },
        None => waft::WindowsWindowConfig::default(),
    }
}

fn level_of(level: WindowLevel) -> waft::WindowLevel {
    match level {
        WindowLevel::Normal => waft::WindowLevel::Normal,
        WindowLevel::AlwaysOnTop => waft::WindowLevel::AlwaysOnTop,
        WindowLevel::AlwaysOnBottom => waft::WindowLevel::AlwaysOnBottom,
    }
}

fn background_of(background: WindowBackground) -> waft::WindowBackground {
    match background {
        WindowBackground::Solid => waft::WindowBackground::Solid,
        WindowBackground::Transparent => waft::WindowBackground::Transparent,
        WindowBackground::Blurred => waft::WindowBackground::Blurred,
        WindowBackground::Glass => waft::WindowBackground::Glass,
    }
}

#[cfg(test)]
mod tests {
    use waos::bindings::wasdk::platform::windows::{MacosTitleBar, Point, Size};

    use super::*;

    fn face(name: &str) -> ProgramFace {
        ProgramFace {
            name: name.to_owned(),
            icon: None,
            identity: None,
        }
    }

    #[test]
    fn an_ordinary_untitled_window_has_the_programs_name_and_the_platforms_size() {
        let window = window_config_of(&WindowConfig::default(), &face("Triangle"), None);
        assert_eq!(window.title, "Triangle");
        assert_eq!(window.size, waft::WindowConfig::default().size);
        assert_eq!(window.position, None);
        assert!(window.decorations && window.resizable && window.visible);
        assert!(window.activating && window.shadow && !window.all_desktops);
        assert_eq!(window.level, waft::WindowLevel::Normal);
        assert_eq!(window.background, waft::WindowBackground::Solid);
        assert_eq!(window.macos.title_bar, waft::MacosTitleBar::Standard);
        assert_eq!(window.windows, waft::WindowsWindowConfig::default());
        assert_eq!(window.linux, waft::LinuxWindowConfig::default());
    }

    #[test]
    fn a_window_is_its_programs_app_on_windows_and_linux() {
        let relaunch = waft::WindowsRelaunch {
            command: r#""C:\WaOS\waos.exe" open --program dev.example.notes"#.to_owned(),
            display_name: "Notes".to_owned(),
            icon: Some(waft::WindowsIconResource {
                path: r"C:\WaOS\icons\dev.example.notes.ico".into(),
                index: 0,
            }),
        };
        let face = ProgramFace {
            name: "Notes".to_owned(),
            icon: None,
            identity: Some(ProgramIdentity {
                app_id: "dev.waos.app.dev.example.notes".to_owned(),
                relaunch: Some(relaunch.clone()),
            }),
        };
        let token = Some("gnome-shell/Notes/1".to_owned());
        let window = window_config_of(&WindowConfig::default(), &face, token.clone());
        assert_eq!(
            window.windows,
            waft::WindowsWindowConfig {
                app_user_model_id: Some("dev.waos.app.dev.example.notes".to_owned()),
                relaunch: Some(relaunch),
            }
        );
        assert_eq!(
            window.linux,
            waft::LinuxWindowConfig {
                app_id: Some("dev.waos.app.dev.example.notes".to_owned()),
                activation_token: token,
            }
        );
    }

    #[test]
    fn a_window_carries_its_programs_icon() {
        let icon = waft::ui::IconImage {
            width: 1,
            height: 1,
            rgba: std::sync::Arc::from([0x3b, 0x82, 0xf6, 0xff].as_slice()),
            template: false,
        };
        let face = ProgramFace {
            name: "Gallery".to_owned(),
            icon: Some(icon.clone()),
            identity: None,
        };
        let window = window_config_of(&WindowConfig::default(), &face, None);
        assert_eq!(window.icon, Some(icon));
    }

    #[test]
    fn every_option_the_program_set_is_the_windows() {
        let config = WindowConfig {
            title: "Panel".to_owned(),
            size: Some(Size {
                width: 320.0,
                height: 200.0,
            }),
            position: Some(Point { x: 12.0, y: 34.0 }),
            decorations: false,
            resizable: false,
            level: WindowLevel::AlwaysOnTop,
            activating: false,
            all_desktops: true,
            background: WindowBackground::Blurred,
            shadow: false,
            visible: false,
            macos_title_bar: MacosTitleBar::Hidden(40.0),
        };
        let window = window_config_of(&config, &face("Survey"), None);
        assert_eq!(window.title, "Panel");
        assert_eq!(window.size, [320.0, 200.0]);
        assert_eq!(window.position, Some([12.0, 34.0]));
        assert!(!window.decorations && !window.resizable && !window.visible);
        assert!(!window.activating && window.all_desktops && !window.shadow);
        assert_eq!(window.level, waft::WindowLevel::AlwaysOnTop);
        assert_eq!(window.background, waft::WindowBackground::Blurred);
        assert_eq!(
            window.macos.title_bar,
            waft::MacosTitleBar::Hidden { height: 40.0 }
        );
    }
}
