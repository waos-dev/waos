//! `wasdk:platform/windows`: what a desktop does around a view, as the engine keeps it: how a
//! program asks for its window ([`WindowConfig`], the record of what it set on its
//! `window-config`), a window's frame, whether it is maximized and what the system's settings say a
//! double-click on a title bar does, as the shell last reported each, and its title as the program
//! set it last ([`ProgramWindow`]), beside the shell's handle the program's requests go to, and the
//! window's size limits, where the shell has them ([`SizeLimits`]).
//!
//! The invariant: what the program reads of its window is the shell's report, never what the
//! program asked for. `set-frame` and `set-maximized` go to the shell, and the program reads the
//! new frame, or that the window is maximized, once the shell reports it. The interface is served
//! to a program by [`host`].

pub(crate) mod host;

use std::sync::{Arc, Mutex};

use crate::bindings::wasdk::platform::windows::{
    MacosTitleBar, Point, Rect, Size, TitleBarAction, WindowBackground, WindowLevel,
};
use crate::shell::{PlacedWindow, ShellWindow, ShellWindowSizeLimits};
use crate::sync::locked;

/// How a program asked for its window: what it set on its `window-config`, each option at the
/// platform's ordinary value unless set. The shell makes the window from it.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Empty unless set.
    pub title: String,
    /// The platform's default size unless set.
    pub size: Option<Size>,
    /// Where the window opens; the platform chooses unless set.
    pub position: Option<Point>,
    /// The system's title bar and border.
    pub decorations: bool,
    pub resizable: bool,
    pub level: WindowLevel,
    /// Whether showing or clicking the window makes it the focused one.
    pub activating: bool,
    /// Whether the window is on every virtual desktop.
    pub all_desktops: bool,
    pub background: WindowBackground,
    /// Whether the window casts the system's shadow.
    pub shadow: bool,
    pub visible: bool,
    /// macOS only: the window's title bar.
    pub macos_title_bar: MacosTitleBar,
}

impl Default for WindowConfig {
    /// The platform's ordinary window, as the WIT's constructor describes it.
    fn default() -> WindowConfig {
        WindowConfig {
            title: String::new(),
            size: None,
            position: None,
            decorations: true,
            resizable: true,
            level: WindowLevel::Normal,
            activating: true,
            all_desktops: false,
            background: WindowBackground::Solid,
            shadow: true,
            visible: true,
            macos_title_bar: MacosTitleBar::Standard,
        }
    }
}

/// One window of the program's, shared by the shell, which reports its frame, whether it is
/// maximized and what a double-click on a title bar does, and the engine, which reads them and
/// carries the program's requests to the shell. Every clone is the same window.
#[derive(Clone)]
pub struct ProgramWindow(Arc<WindowState>);

struct WindowState {
    shell: Arc<dyn ShellWindow>,
    frame: Mutex<Rect>,
    maximized: Mutex<bool>,
    title_bar_double_click_action: Mutex<Option<TitleBarAction>>,
    /// As the program set it last, on its `window-config` or on the window; empty for none.
    title: Mutex<String>,
}

impl ProgramWindow {
    /// The window `placed`, titled `title` as the program asked for it.
    pub(crate) fn new(placed: PlacedWindow, title: String) -> ProgramWindow {
        ProgramWindow(Arc::new(WindowState {
            shell: placed.window,
            frame: Mutex::new(placed.frame),
            maximized: Mutex::new(placed.maximized),
            title_bar_double_click_action: Mutex::new(placed.title_bar_double_click_action),
            title: Mutex::new(title),
        }))
    }

    /// The title the program set last; empty for a window it left untitled, which the shell
    /// titles after the program.
    pub(crate) fn title(&self) -> String {
        locked(&self.0.title).clone()
    }

    /// The program titles the window `title`: kept, and carried to the shell.
    pub(crate) fn set_title(&self, title: String) {
        self.shell().set_title(&title);
        *locked(&self.0.title) = title;
    }

    /// Where the window is on the desktop, in logical points from the top-left of the main
    /// screen, y down, as the shell last reported it.
    pub(crate) fn frame(&self) -> Rect {
        *locked(&self.0.frame)
    }

    /// Sizes the window so the view it holds, `view` logical points now, is `size`, its top-left
    /// where it is: the frame grown by what the window holds around the view, a title bar and a
    /// border, which a size leaves as they are. Carried to the shell at once, unanimated, as a
    /// program's `set-frame` is.
    pub(crate) fn size_view(&self, view: Size, size: Size) {
        let frame = self.frame();
        let around = |outer: f64, inner: f64| (outer - inner).max(0.0);
        let sized = Rect {
            origin: frame.origin,
            size: Size {
                width: size.width + around(frame.size.width, view.width),
                height: size.height + around(frame.size.height, view.height),
            },
        };
        self.shell().set_frame(sized, None);
    }

    /// The shell reports the window's frame is `frame` now.
    pub(crate) fn frame_changed(&self, frame: Rect) {
        *locked(&self.0.frame) = frame;
    }

    /// Whether the window is maximized, on macOS zoomed, as the shell last reported it.
    pub(crate) fn is_maximized(&self) -> bool {
        *locked(&self.0.maximized)
    }

    /// The shell reports the window is maximized now, or no longer.
    pub(crate) fn maximized_changed(&self, maximized: bool) {
        *locked(&self.0.maximized) = maximized;
    }

    /// What the system's settings say a double-click on a title bar does, as the shell last
    /// reported it; none where the system has no such setting.
    pub(crate) fn title_bar_double_click_action(&self) -> Option<TitleBarAction> {
        *locked(&self.0.title_bar_double_click_action)
    }

    /// The shell reports the system's settings say a double-click on a title bar does `action`
    /// now.
    pub(crate) fn title_bar_double_click_action_changed(&self, action: Option<TitleBarAction>) {
        *locked(&self.0.title_bar_double_click_action) = action;
    }

    /// The shell's side, which the program's requests go to.
    pub(crate) fn shell(&self) -> &dyn ShellWindow {
        self.0.shell.as_ref()
    }

    /// The window's size limits; none where the shell has none.
    pub(crate) fn size_limits(&self) -> Option<SizeLimits> {
        self.shell()
            .size_limits()
            .is_some()
            .then(|| SizeLimits(self.clone()))
    }
}

/// `windows.size-limits`: a window's size limits, which the program's requests go to through the
/// shell's window, whose answer stays the same while the window is open.
pub struct SizeLimits(ProgramWindow);

impl SizeLimits {
    pub(crate) fn set_min_size(&self, size: Option<Size>) {
        if let Some(limits) = self.shell() {
            limits.set_min_size(size);
        }
    }

    pub(crate) fn set_max_size(&self, size: Option<Size>) {
        if let Some(limits) = self.shell() {
            limits.set_max_size(size);
        }
    }

    fn shell(&self) -> Option<&dyn ShellWindowSizeLimits> {
        self.0.shell().size_limits()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::shell::tests::Headless;

    pub(crate) fn rect(x: f64) -> Rect {
        Rect {
            origin: Point { x, y: 0.0 },
            size: Size {
                width: 1.0,
                height: 1.0,
            },
        }
    }

    #[test]
    fn a_new_config_is_the_platforms_ordinary_window() {
        let config = WindowConfig::default();
        assert!(config.title.is_empty());
        assert!(config.size.is_none() && config.position.is_none());
        assert!(config.decorations && config.resizable && config.activating);
        assert!(config.shadow && config.visible && !config.all_desktops);
        assert!(matches!(config.level, WindowLevel::Normal));
        assert!(matches!(config.background, WindowBackground::Solid));
        assert!(matches!(config.macos_title_bar, MacosTitleBar::Standard));
    }

    /// A shell's window with size limits, which keeps the last of each it was given.
    #[derive(Default)]
    struct Limited {
        min: Mutex<Option<Size>>,
        max: Mutex<Option<Size>>,
    }

    impl ShellWindow for Limited {
        fn set_frame(&self, _frame: Rect, _animate: Option<std::time::Duration>) {}

        fn set_title(&self, _title: &str) {}

        fn show(&self) {}

        fn hide(&self) {}

        fn close(&self) {}

        fn size_limits(&self) -> Option<&dyn ShellWindowSizeLimits> {
            Some(self)
        }

        fn drag(&self) {}

        fn set_maximized(&self, _maximized: bool) {}

        fn set_minimized(&self, _minimized: bool) {}
    }

    impl ShellWindowSizeLimits for Limited {
        fn set_min_size(&self, size: Option<Size>) {
            *locked(&self.min) = size;
        }

        fn set_max_size(&self, size: Option<Size>) {
            *locked(&self.max) = size;
        }
    }

    /// A window placed with `window` as its shell's side, at a frame of its own, neither
    /// maximized nor on a system with a setting for a double-click on a title bar.
    pub(crate) fn placed_window(window: Arc<dyn ShellWindow>) -> PlacedWindow {
        PlacedWindow {
            window,
            frame: rect(1.0),
            maximized: false,
            title_bar_double_click_action: None,
        }
    }

    fn placed(window: Arc<dyn ShellWindow>) -> ProgramWindow {
        ProgramWindow::new(placed_window(window), "Notes".to_owned())
    }

    #[test]
    fn a_window_has_size_limits_only_where_the_shells_window_has_them() {
        assert!(
            placed(Arc::new(Headless::default()))
                .size_limits()
                .is_none()
        );
        assert!(placed(Arc::new(Limited::default())).size_limits().is_some());
    }

    #[test]
    fn size_limits_go_to_the_shells_window_each_on_its_own() {
        let shell = Arc::new(Limited::default());
        let limits = placed(Arc::clone(&shell) as Arc<dyn ShellWindow>)
            .size_limits()
            .unwrap();
        let size = Size {
            width: 320.0,
            height: 240.0,
        };
        limits.set_min_size(Some(size));
        limits.set_max_size(Some(size));
        limits.set_max_size(None);
        let kept =
            |limit: &Mutex<Option<Size>>| locked(limit).map(|size| (size.width, size.height));
        assert_eq!(kept(&shell.min), Some((320.0, 240.0)));
        assert_eq!(kept(&shell.max), None, "taken away on its own");
    }

    #[test]
    fn a_view_sized_keeps_the_windows_top_left_and_what_it_holds_around_the_view() {
        let shell = Arc::new(Headless::default());
        let frame = Rect {
            origin: Point { x: 10.0, y: 20.0 },
            size: Size {
                width: 400.0,
                height: 328.0,
            },
        };
        let window = ProgramWindow::new(
            PlacedWindow {
                frame,
                ..placed_window(Arc::clone(&shell) as Arc<dyn ShellWindow>)
            },
            String::new(),
        );
        let view = Size {
            width: 400.0,
            height: 300.0,
        };
        let phone = Size {
            width: 390.0,
            height: 844.0,
        };
        window.size_view(view, phone);
        let set = shell.frames_set();
        assert_eq!(set.len(), 1);
        assert_eq!((set[0].origin.x, set[0].origin.y), (10.0, 20.0));
        assert_eq!(
            (set[0].size.width, set[0].size.height),
            (390.0, 872.0),
            "the title bar's 28 points kept above the view"
        );
        assert_eq!(window.frame().size.height, 328.0, "until the shell reports");
    }

    #[test]
    fn the_frame_is_what_the_shell_last_reported() {
        let window = placed(Arc::new(Headless::default()));
        assert_eq!(window.frame().origin.x, 1.0);
        window.frame_changed(rect(2.0));
        assert_eq!(window.frame().origin.x, 2.0);
    }

    #[test]
    fn whether_it_is_maximized_is_what_the_shell_last_reported() {
        let window = ProgramWindow::new(
            PlacedWindow {
                maximized: true,
                ..placed_window(Arc::new(Headless::default()))
            },
            String::new(),
        );
        assert!(window.is_maximized(), "as it was placed");
        window.maximized_changed(false);
        assert!(!window.is_maximized());
    }

    #[test]
    fn a_title_bars_double_click_is_what_the_shell_last_reported() {
        let window = ProgramWindow::new(
            PlacedWindow {
                title_bar_double_click_action: Some(TitleBarAction::Maximize),
                ..placed_window(Arc::new(Headless::default()))
            },
            String::new(),
        );
        assert_eq!(
            window.title_bar_double_click_action(),
            Some(TitleBarAction::Maximize)
        );
        window.title_bar_double_click_action_changed(Some(TitleBarAction::Nothing));
        assert_eq!(
            window.title_bar_double_click_action(),
            Some(TitleBarAction::Nothing)
        );
        window.title_bar_double_click_action_changed(None);
        assert_eq!(window.title_bar_double_click_action(), None);
    }

    #[test]
    fn the_title_is_the_one_the_program_set_last() {
        let window = placed(Arc::new(Headless::default()));
        assert_eq!(window.title(), "Notes");
        window.set_title("Shopping".to_owned());
        assert_eq!(window.title(), "Shopping");
    }
}
