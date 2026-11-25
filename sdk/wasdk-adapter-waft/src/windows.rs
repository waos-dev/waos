//! Windows: the one around a view, as Waft's [`HostWindow`], with its size limits where the
//! window has them, its move from a press, maximizing and minimizing, and what a double-click on a
//! title bar does; the configuration a request for a view makes its window with, macOS's title bar
//! among it; and the WIT's `rect`, a window's frame or a screen's, as Waft's.
//!
//! The WIT's `window` does not say whether the window is minimized or occluded, nor that it
//! moved; the framework hears neither (`is_minimized` and `is_occluded` answer false, as on
//! Wayland, the design's decision 35), and a view off screen gets no frames. Whether it is
//! maximized or full screen, and what a double-click on a title bar does, are read from the WIT
//! at each call.

use std::rc::Rc;
use std::time::Duration;

use waft_embedder::{
    HostWindow, IconImage, MacosTitleBar, RawWindowHandle, Rect, ScreenId, TitleBarAction,
    ViewError, ViewRef, WindowBackground, WindowConfig, WindowLevel, WindowSizeLimits,
};
use wasdk::views;
use wasdk::windows::{self, Point, Size};

use crate::views::WasdkView;

/// The window a view is in, as the app holds it.
pub(crate) struct WasdkWindow {
    window: windows::Window,
    view: Rc<WasdkView>,
    /// The window's size limits, which the WIT answers alike while the window is open; none where
    /// users do not resize windows.
    size_limits: Option<windows::SizeLimits>,
}

impl WasdkWindow {
    /// The app's handle on `window`, which `view` is in: the view answers it as its window from
    /// now on.
    pub(crate) fn around(window: windows::Window, view: &Rc<WasdkView>) -> Rc<WasdkWindow> {
        let size_limits = window.size_limits();
        let window = Rc::new(WasdkWindow {
            window,
            view: Rc::clone(view),
            size_limits,
        });
        view.set_window(&window);
        window
    }
}

impl HostWindow for WasdkWindow {
    fn view(&self) -> ViewRef {
        Rc::clone(&self.view) as ViewRef
    }

    fn frame(&self) -> Rect {
        rect_of(self.window.frame())
    }

    fn set_frame(&self, frame: Rect, animate: Option<Duration>) {
        let animate =
            animate.map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX));
        self.window.set_frame(
            windows::Rect {
                origin: Point {
                    x: frame.left,
                    y: frame.top,
                },
                size: Size {
                    width: frame.width(),
                    height: frame.height(),
                },
            },
            animate,
        );
    }

    fn set_title(&self, title: &str) {
        self.window.set_title(title);
    }

    /// The WIT gives a window no icon of its own: a program's windows carry the program's, from
    /// its package.
    fn set_icon(&self, _icon: Option<&IconImage>) {}

    fn show(&self) {
        self.window.show();
    }

    fn hide(&self) {
        self.window.hide();
    }

    fn is_minimized(&self) -> bool {
        false
    }

    fn is_occluded(&self) -> bool {
        false
    }

    /// Closes the window. The view goes with it, and the loop reports it removed when
    /// `view-changes` says so.
    fn close(&self) {
        self.window.close();
    }

    fn size_limits(&self) -> Option<&dyn WindowSizeLimits> {
        self.size_limits
            .is_some()
            .then_some(self as &dyn WindowSizeLimits)
    }

    fn drag(&self) {
        self.window.drag();
    }

    fn is_maximized(&self) -> bool {
        self.window.is_maximized()
    }

    fn set_maximized(&self, maximized: bool) {
        self.window.set_maximized(maximized);
    }

    fn set_minimized(&self, minimized: bool) {
        self.window.set_minimized(minimized);
    }

    fn is_fullscreen(&self) -> bool {
        self.window.is_fullscreen()
    }

    fn set_fullscreen(&self, fullscreen: bool, screen: Option<ScreenId>) {
        self.window
            .set_fullscreen(fullscreen, screen.map(|screen| screen.0));
    }

    /// `Unknown` where the WIT answers none, the system having no such setting.
    fn title_bar_double_click_action(&self) -> TitleBarAction {
        title_bar_action_of(self.window.title_bar_double_click_action())
    }

    /// None: the WIT carries no native window, whose system may not even be this process's.
    fn native_handle(&self) -> Option<RawWindowHandle> {
        None
    }
}

/// The WIT's `size-limits`, which Waft only reaches where the window has them.
impl WindowSizeLimits for WasdkWindow {
    fn set_min_size(&self, size: Option<waft_embedder::Size>) {
        if let Some(limits) = &self.size_limits {
            limits.set_min_size(size.map(size_of));
        }
    }

    fn set_max_size(&self, size: Option<waft_embedder::Size>) {
        if let Some(limits) = &self.size_limits {
            limits.set_max_size(size.map(size_of));
        }
    }
}

/// Waft's size as the WIT's: both in logical points.
fn size_of(size: waft_embedder::Size) -> Size {
    Size {
        width: size.width(),
        height: size.height(),
    }
}

/// `rect`, a window's frame or a screen's, as Waft's: both in logical points from the top-left
/// of the main screen.
pub(crate) fn rect_of(rect: windows::Rect) -> Rect {
    Rect::from_ltwh(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    )
}

/// Waft's configuration as the `window-config` a request takes: every option set, under the
/// same names.
pub(crate) fn window_config(config: &WindowConfig) -> windows::WindowConfig {
    let window = windows::WindowConfig::new();
    window.set_title(&config.title);
    window.set_size(Size {
        width: config.size[0],
        height: config.size[1],
    });
    if let Some([x, y]) = config.position {
        window.set_position(Point { x, y });
    }
    window.set_decorations(config.decorations);
    window.set_resizable(config.resizable);
    window.set_level(window_level(config.level));
    window.set_activating(config.activating);
    window.set_all_desktops(config.all_desktops);
    window.set_background(window_background(config.background));
    window.set_shadow(config.shadow);
    window.set_visible(config.visible);
    window.set_macos_title_bar(macos_title_bar(config.macos.title_bar));
    window
}

fn macos_title_bar(title_bar: MacosTitleBar) -> windows::MacosTitleBar {
    match title_bar {
        MacosTitleBar::Standard => windows::MacosTitleBar::Standard,
        MacosTitleBar::Hidden { height } => windows::MacosTitleBar::Hidden(height),
    }
}

/// What the WIT says a double-click on a title bar does, in Waft's words: `Unknown` where the
/// system has no such setting.
fn title_bar_action_of(action: Option<windows::TitleBarAction>) -> TitleBarAction {
    match action {
        Some(windows::TitleBarAction::Maximize) => TitleBarAction::Maximize,
        Some(windows::TitleBarAction::Minimize) => TitleBarAction::Minimize,
        Some(windows::TitleBarAction::Nothing) => TitleBarAction::Nothing,
        None => TitleBarAction::Unknown,
    }
}

fn window_level(level: WindowLevel) -> windows::WindowLevel {
    match level {
        WindowLevel::Normal => windows::WindowLevel::Normal,
        WindowLevel::AlwaysOnTop => windows::WindowLevel::AlwaysOnTop,
        WindowLevel::AlwaysOnBottom => windows::WindowLevel::AlwaysOnBottom,
    }
}

fn window_background(background: WindowBackground) -> windows::WindowBackground {
    match background {
        WindowBackground::Solid => windows::WindowBackground::Solid,
        WindowBackground::Transparent => windows::WindowBackground::Transparent,
        WindowBackground::Blurred => windows::WindowBackground::Blurred,
        WindowBackground::Glass => windows::WindowBackground::Glass,
    }
}

/// What a frame of a view in a window with `background` starts from: white, or nothing where
/// what is behind the window shows through what the app leaves unpainted.
pub(crate) fn clear_of(background: WindowBackground) -> valo::Color {
    if background.sees_through() {
        valo::Color::TRANSPARENT
    } else {
        valo::Color::WHITE
    }
}

/// Why a request got no view, in Waft's words.
pub(crate) fn view_error(error: views::ViewError) -> ViewError {
    match error {
        views::ViewError::Unsupported => ViewError::Unsupported,
        views::ViewError::Failed(reason) => ViewError::Failed(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rect_keeps_its_place_and_size() {
        let rect = rect_of(windows::Rect {
            origin: Point {
                x: -1920.0,
                y: 25.0,
            },
            size: Size {
                width: 1920.0,
                height: 1055.0,
            },
        });
        assert_eq!(rect, Rect::from_ltwh(-1920.0, 25.0, 1920.0, 1055.0));
    }

    #[test]
    fn every_level_and_background_keeps_its_name() {
        assert_eq!(
            window_level(WindowLevel::AlwaysOnTop),
            windows::WindowLevel::AlwaysOnTop
        );
        assert_eq!(
            window_level(WindowLevel::AlwaysOnBottom),
            windows::WindowLevel::AlwaysOnBottom
        );
        assert_eq!(
            window_background(WindowBackground::Blurred),
            windows::WindowBackground::Blurred
        );
        assert_eq!(
            window_background(WindowBackground::Glass),
            windows::WindowBackground::Glass
        );
    }

    #[test]
    fn a_macos_title_bar_keeps_its_band() {
        assert!(matches!(
            macos_title_bar(MacosTitleBar::Standard),
            windows::MacosTitleBar::Standard
        ));
        assert!(matches!(
            macos_title_bar(MacosTitleBar::Hidden { height: 52.0 }),
            windows::MacosTitleBar::Hidden(52.0)
        ));
    }

    #[test]
    fn a_title_bars_double_click_keeps_its_name_and_none_is_unknown() {
        assert_eq!(
            title_bar_action_of(Some(windows::TitleBarAction::Maximize)),
            TitleBarAction::Maximize
        );
        assert_eq!(
            title_bar_action_of(Some(windows::TitleBarAction::Minimize)),
            TitleBarAction::Minimize
        );
        assert_eq!(
            title_bar_action_of(Some(windows::TitleBarAction::Nothing)),
            TitleBarAction::Nothing
        );
        assert_eq!(title_bar_action_of(None), TitleBarAction::Unknown);
    }

    #[test]
    fn only_a_solid_window_starts_its_frames_from_white() {
        assert_eq!(clear_of(WindowBackground::Solid), valo::Color::WHITE);
        for background in [
            WindowBackground::Transparent,
            WindowBackground::Blurred,
            WindowBackground::Glass,
        ] {
            assert_eq!(clear_of(background), valo::Color::TRANSPARENT);
        }
    }

    #[test]
    fn a_platform_that_makes_no_views_is_unsupported_rather_than_a_failure() {
        assert_eq!(
            view_error(views::ViewError::Unsupported),
            ViewError::Unsupported
        );
        assert_eq!(
            view_error(views::ViewError::Failed(String::from("no display"))),
            ViewError::Failed(String::from("no display"))
        );
    }
}
