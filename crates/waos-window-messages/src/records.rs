//! How the program's records, and the engine's values beside them, are written in a message:
//! serde's remote definitions of the types wasmtime generates from the WIT, which take no derive of
//! serde's (`waos::bindings`), so the messages carry the records themselves.
//!
//! Each definition names every field and variant of its record, and serde checks it against the
//! record when it compiles: a field or variant the WIT gains fails the build here, until it is
//! written too. The invariant: a definition is the record's shape, field for field, in its order;
//! nothing here converts.

use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use waos::bindings::wasdk::platform::file_dialogs::DialogError;
use waos::bindings::wasdk::platform::lifecycle::AppExitResponse;
use waos::bindings::wasdk::platform::menus::{
    CheckState, Image, ItemCheck, MenuEntry, MenuEntryKind, MenuItem, Provided, ProvidedItem,
    Shortcut, ShortcutModifiers, Submenu,
};
use waos::bindings::wasdk::platform::notifications::{Activation, NotificationError};
use waos::bindings::wasdk::platform::system::Brightness;
use waos::bindings::wasdk::platform::views::{
    Cursor, EditCommand, Insets, KeyDevice, KeyEvent, KeyKind, Metrics, NumberFormat, OverlayStyle,
    PixelRect, PointerButtons, PointerChange, PointerDeviceKind, PointerEvent, PointerSignalKind,
    TextAffinity, TextCapitalization, TextEditingValue, TextInputAction, TextInputConfiguration,
    TextInputType, TextRange, TextSelection, ViewError, ViewFocusDirection, ViewFocusEvent,
    ViewFocusState,
};
use waos::bindings::wasdk::platform::windows::{
    MacosTitleBar, Point, Rect, Size, TitleBarAction, WindowBackground, WindowLevel,
};
use waos::{
    FileType, FolderOptions, Notification, NotificationAction, OpenOptions, OpenRequest, Presence,
    SaveOptions, SystemPermissionState, WindowConfig,
};

/// An optional record, written as serde writes an option of it.
macro_rules! optional {
    ($module:ident, $record:ty, $definition:literal) => {
        pub(crate) mod $module {
            use super::*;

            pub(crate) fn serialize<S: Serializer>(
                value: &Option<$record>,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                #[derive(Serialize)]
                struct Written<'a>(#[serde(with = $definition)] &'a $record);
                value.as_ref().map(Written).serialize(serializer)
            }

            pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Option<$record>, D::Error> {
                #[derive(Deserialize)]
                struct Read(#[serde(with = $definition)] $record);
                Ok(Option::<Read>::deserialize(deserializer)?.map(|Read(record)| record))
            }
        }
    };
}

optional!(option_text_selection, TextSelection, "TextSelectionDef");
optional!(option_text_range, TextRange, "TextRangeDef");
optional!(option_size, Size, "SizeDef");
optional!(option_point, Point, "PointDef");
optional!(option_shortcut, Shortcut, "ShortcutDef");
optional!(option_image, Image, "ImageDef");
optional!(option_title_bar_action, TitleBarAction, "TitleBarActionDef");
optional!(option_brightness, Brightness, "BrightnessDef");

/// A list of records, written as serde writes a list of them.
macro_rules! listed {
    ($module:ident, $record:ty, $definition:literal) => {
        pub(crate) mod $module {
            use super::*;

            pub(crate) fn serialize<S: Serializer>(
                records: &[$record],
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                #[derive(Serialize)]
                struct Written<'a>(#[serde(with = $definition)] &'a $record);
                serializer.collect_seq(records.iter().map(Written))
            }

            pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<Vec<$record>, D::Error> {
                #[derive(Deserialize)]
                struct Read(#[serde(with = $definition)] $record);
                let read = Vec::<Read>::deserialize(deserializer)?;
                Ok(read.into_iter().map(|Read(record)| record).collect())
            }
        }
    };
}

listed!(menu_entries, MenuEntry, "MenuEntryDef");
listed!(file_types, FileType, "FileTypeDef");
listed!(
    notification_actions,
    NotificationAction,
    "NotificationActionDef"
);

/// What a file dialog answers, written as serde writes a result of it: the paths picked, or why
/// none were.
pub(crate) mod picked_paths {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        picked: &Result<Vec<PathBuf>, DialogError>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Written<'a>(#[serde(with = "DialogErrorDef")] &'a DialogError);
        picked.as_ref().map_err(Written).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Result<Vec<PathBuf>, DialogError>, D::Error> {
        #[derive(Deserialize)]
        struct Read(#[serde(with = "DialogErrorDef")] DialogError);
        let read = Result::<Vec<PathBuf>, Read>::deserialize(deserializer)?;
        Ok(read.map_err(|Read(error)| error))
    }
}

/// What a post or a schedule answers, written as serde writes a result of it: the system has the
/// notification, or why not.
pub(crate) mod posted {
    use super::*;

    pub(crate) fn serialize<S: Serializer>(
        posted: &Result<(), NotificationError>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Written<'a>(#[serde(with = "NotificationErrorDef")] &'a NotificationError);
        posted.as_ref().map_err(Written).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Result<(), NotificationError>, D::Error> {
        #[derive(Deserialize)]
        struct Read(#[serde(with = "NotificationErrorDef")] NotificationError);
        let read = Result::<(), Read>::deserialize(deserializer)?;
        Ok(read.map_err(|Read(error)| error))
    }
}

/// A flags type, as the bits the WIT counts its flags in: its first flag is the low bit.
macro_rules! flags {
    ($(#[$doc:meta])* $module:ident, $flags:ident, [$($flag:ident),+ $(,)?]) => {
        $(#[$doc])*
        pub(crate) mod $module {
            use super::*;

            const FLAGS: &[$flags] = &[$($flags::$flag),+];

            pub(crate) fn serialize<S: Serializer>(
                held: &$flags,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let bits = FLAGS
                    .iter()
                    .enumerate()
                    .filter(|(_, flag)| held.contains(**flag))
                    .fold(0u8, |bits, (place, _)| bits | 1 << place);
                bits.serialize(serializer)
            }

            pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$flags, D::Error> {
                let bits = u8::deserialize(deserializer)?;
                let held = FLAGS
                    .iter()
                    .enumerate()
                    .filter(|(place, _)| bits & 1 << place != 0)
                    .fold($flags::empty(), |held, (_, flag)| held | *flag);
                Ok(held)
            }
        }
    };
}

flags!(
    /// The buttons held: the primary button is the low bit.
    pointer_buttons,
    PointerButtons,
    [PRIMARY, SECONDARY, TERTIARY, BACK, FORWARD]
);
flags!(
    /// The modifier keys a shortcut holds: ⌘, or the Windows key, is the low bit.
    shortcut_modifiers,
    ShortcutModifiers,
    [META, CONTROL, ALT, SHIFT]
);

#[derive(Serialize, Deserialize)]
#[serde(remote = "Cursor")]
pub(crate) enum CursorDef {
    None,
    Default,
    ContextMenu,
    Help,
    Pointer,
    Progress,
    Wait,
    Cell,
    Crosshair,
    Text,
    VerticalText,
    Alias,
    Copy,
    Move,
    NoDrop,
    NotAllowed,
    Grab,
    Grabbing,
    EResize,
    NResize,
    NeResize,
    NwResize,
    SResize,
    SeResize,
    SwResize,
    WResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    ZoomIn,
    ZoomOut,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "AppExitResponse")]
pub(crate) enum AppExitResponseDef {
    Exit,
    Cancel,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "SystemPermissionState")]
pub(crate) enum SystemPermissionStateDef {
    NotAsked,
    Allowed,
    Denied,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ViewFocusState")]
pub(crate) enum ViewFocusStateDef {
    Unfocused,
    Focused,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ViewFocusDirection")]
pub(crate) enum ViewFocusDirectionDef {
    Undefined,
    Forward,
    Backward,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Brightness")]
pub(crate) enum BrightnessDef {
    Light,
    Dark,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "OverlayStyle")]
pub(crate) struct OverlayStyleDef {
    #[serde(with = "option_brightness")]
    status_bar_icons: Option<Brightness>,
    #[serde(with = "option_brightness")]
    navigation_bar_icons: Option<Brightness>,
    status_bar_contrast_enforced: Option<bool>,
    navigation_bar_contrast_enforced: Option<bool>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ViewFocusEvent")]
pub(crate) struct ViewFocusEventDef {
    #[serde(with = "ViewFocusStateDef")]
    state: ViewFocusState,
    #[serde(with = "ViewFocusDirectionDef")]
    direction: ViewFocusDirection,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextEditingValue")]
pub(crate) struct TextEditingValueDef {
    text: String,
    #[serde(with = "option_text_selection")]
    selection: Option<TextSelection>,
    #[serde(with = "option_text_range")]
    composing: Option<TextRange>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextSelection")]
pub(crate) struct TextSelectionDef {
    base_offset: u32,
    extent_offset: u32,
    #[serde(with = "TextAffinityDef")]
    affinity: TextAffinity,
    is_directional: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextRange")]
pub(crate) struct TextRangeDef {
    start: u32,
    end: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextAffinity")]
pub(crate) enum TextAffinityDef {
    Upstream,
    Downstream,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextInputConfiguration")]
pub(crate) struct TextInputConfigurationDef {
    #[serde(with = "TextInputTypeDef")]
    input_type: TextInputType,
    #[serde(with = "TextInputActionDef")]
    input_action: TextInputAction,
    obscure_text: bool,
    autocorrect: bool,
    enable_suggestions: bool,
    #[serde(with = "TextCapitalizationDef")]
    text_capitalization: TextCapitalization,
    read_only: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextInputType")]
pub(crate) enum TextInputTypeDef {
    Text,
    Multiline,
    Number(#[serde(with = "NumberFormatDef")] NumberFormat),
    Phone,
    Datetime,
    EmailAddress,
    Url,
    VisiblePassword,
    Name,
    StreetAddress,
    None,
    WebSearch,
    Twitter,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "NumberFormat")]
pub(crate) struct NumberFormatDef {
    signed: bool,
    decimal: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextInputAction")]
pub(crate) enum TextInputActionDef {
    None,
    Unspecified,
    Done,
    Go,
    Search,
    Send,
    Next,
    Previous,
    ContinueAction,
    Join,
    Route,
    EmergencyCall,
    Newline,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TextCapitalization")]
pub(crate) enum TextCapitalizationDef {
    Words,
    Sentences,
    Characters,
    None,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PixelRect")]
pub(crate) struct PixelRectDef {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "EditCommand")]
pub(crate) enum EditCommandDef {
    Undo,
    Redo,
    SelectAll,
    Delete,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Metrics")]
pub(crate) struct MetricsDef {
    width: u32,
    height: u32,
    device_pixel_ratio: f64,
    #[serde(with = "InsetsDef")]
    padding: Insets,
    #[serde(with = "InsetsDef")]
    view_padding: Insets,
    #[serde(with = "InsetsDef")]
    view_insets: Insets,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Insets")]
pub(crate) struct InsetsDef {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PointerEvent")]
pub(crate) struct PointerEventDef {
    time: u64,
    #[serde(with = "PointerChangeDef")]
    change: PointerChange,
    #[serde(with = "PointerDeviceKindDef")]
    kind: PointerDeviceKind,
    #[serde(with = "PointerSignalKindDef")]
    signal: PointerSignalKind,
    device: i64,
    x: f64,
    y: f64,
    #[serde(with = "pointer_buttons")]
    buttons: PointerButtons,
    pressure: f64,
    pressure_min: f64,
    pressure_max: f64,
    distance: f64,
    distance_max: f64,
    size: f64,
    radius_major: f64,
    radius_minor: f64,
    radius_min: f64,
    radius_max: f64,
    orientation: f64,
    tilt: f64,
    scroll_delta_x: f64,
    scroll_delta_y: f64,
    pan_x: f64,
    pan_y: f64,
    scale: f64,
    rotation: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PointerChange")]
pub(crate) enum PointerChangeDef {
    Cancel,
    Add,
    Remove,
    Hover,
    Down,
    Move,
    Up,
    PanZoomStart,
    PanZoomUpdate,
    PanZoomEnd,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PointerDeviceKind")]
pub(crate) enum PointerDeviceKindDef {
    Touch,
    Mouse,
    Stylus,
    InvertedStylus,
    Trackpad,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "PointerSignalKind")]
pub(crate) enum PointerSignalKindDef {
    None,
    Scroll,
    ScrollInertiaCancel,
    Scale,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "KeyEvent")]
pub(crate) struct KeyEventDef {
    time: u64,
    #[serde(with = "KeyKindDef")]
    kind: KeyKind,
    #[serde(with = "KeyDeviceDef")]
    device: KeyDevice,
    physical: u64,
    logical: u64,
    character: Option<String>,
    synthesized: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "KeyKind")]
pub(crate) enum KeyKindDef {
    Down,
    Up,
    Repeat,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "KeyDevice")]
pub(crate) enum KeyDeviceDef {
    Keyboard,
    DirectionalPad,
    Gamepad,
    Joystick,
    Hdmi,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ViewError")]
pub(crate) enum ViewErrorDef {
    Unsupported,
    Failed(String),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Rect")]
pub(crate) struct RectDef {
    #[serde(with = "PointDef")]
    origin: Point,
    #[serde(with = "SizeDef")]
    size: Size,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Point")]
pub(crate) struct PointDef {
    x: f64,
    y: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Size")]
pub(crate) struct SizeDef {
    width: f64,
    height: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "WindowLevel")]
pub(crate) enum WindowLevelDef {
    Normal,
    AlwaysOnTop,
    AlwaysOnBottom,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "WindowBackground")]
pub(crate) enum WindowBackgroundDef {
    Solid,
    Transparent,
    Blurred,
    Glass,
}

/// The engine's record of what a program set on its `window-config`.
#[derive(Serialize, Deserialize)]
#[serde(remote = "WindowConfig")]
pub(crate) struct WindowConfigDef {
    title: String,
    #[serde(with = "option_size")]
    size: Option<Size>,
    #[serde(with = "option_point")]
    position: Option<Point>,
    decorations: bool,
    resizable: bool,
    #[serde(with = "WindowLevelDef")]
    level: WindowLevel,
    activating: bool,
    all_desktops: bool,
    #[serde(with = "WindowBackgroundDef")]
    background: WindowBackground,
    shadow: bool,
    visible: bool,
    #[serde(with = "MacosTitleBarDef")]
    macos_title_bar: MacosTitleBar,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "MacosTitleBar")]
pub(crate) enum MacosTitleBarDef {
    Standard,
    Hidden(f64),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "TitleBarAction")]
pub(crate) enum TitleBarActionDef {
    Maximize,
    Minimize,
    Nothing,
}

/// How a view stands on screen, as the engine takes it.
#[derive(Serialize, Deserialize)]
#[serde(remote = "Presence")]
pub(crate) struct PresenceDef {
    on_screen: bool,
    occluded: bool,
    focused: bool,
}

/// The files and links a program is opened with, as the engine takes them.
#[derive(Serialize, Deserialize)]
#[serde(remote = "OpenRequest")]
pub(crate) struct OpenRequestDef {
    files: Vec<std::path::PathBuf>,
    links: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "MenuEntry")]
pub(crate) struct MenuEntryDef {
    id: u32,
    parent: Option<u32>,
    #[serde(with = "MenuEntryKindDef")]
    kind: MenuEntryKind,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "MenuEntryKind")]
pub(crate) enum MenuEntryKindDef {
    Submenu(#[serde(with = "SubmenuDef")] Submenu),
    Item(#[serde(with = "MenuItemDef")] MenuItem),
    Provided(#[serde(with = "ProvidedDef")] Provided),
    Separator,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Submenu")]
pub(crate) struct SubmenuDef {
    label: String,
    enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "MenuItem")]
pub(crate) struct MenuItemDef {
    label: String,
    enabled: bool,
    #[serde(with = "ItemCheckDef")]
    check: ItemCheck,
    #[serde(with = "option_shortcut")]
    shortcut: Option<Shortcut>,
    #[serde(with = "option_image")]
    icon: Option<Image>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ItemCheck")]
pub(crate) enum ItemCheckDef {
    Plain,
    Checkbox(#[serde(with = "CheckStateDef")] CheckState),
    Radio(bool),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CheckState")]
pub(crate) enum CheckStateDef {
    Off,
    On,
    Mixed,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Shortcut")]
pub(crate) struct ShortcutDef {
    logical: u64,
    #[serde(with = "shortcut_modifiers")]
    modifiers: ShortcutModifiers,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Provided")]
pub(crate) struct ProvidedDef {
    #[serde(with = "ProvidedItemDef")]
    item: ProvidedItem,
    enabled: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "ProvidedItem")]
pub(crate) enum ProvidedItemDef {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Minimize,
    Zoom,
    ToggleFullScreen,
    BringAllToFront,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Delete,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Image")]
pub(crate) struct ImageDef {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    template: bool,
}

/// The engine's record of a kind of file a dialog offers.
#[derive(Serialize, Deserialize)]
#[serde(remote = "FileType")]
pub(crate) struct FileTypeDef {
    label: String,
    extensions: Vec<String>,
    mime_types: Vec<String>,
}

/// The engine's record of what an open dialog offers.
#[derive(Serialize, Deserialize)]
#[serde(remote = "OpenOptions")]
pub(crate) struct OpenOptionsDef {
    #[serde(with = "file_types")]
    types: Vec<FileType>,
    initial_folder: Option<PathBuf>,
    confirm_label: Option<String>,
    writable: bool,
}

/// The engine's record of what a folder dialog offers.
#[derive(Serialize, Deserialize)]
#[serde(remote = "FolderOptions")]
pub(crate) struct FolderOptionsDef {
    initial_folder: Option<PathBuf>,
    confirm_label: Option<String>,
    writable: bool,
}

/// The engine's record of what a save dialog offers.
#[derive(Serialize, Deserialize)]
#[serde(remote = "SaveOptions")]
pub(crate) struct SaveOptionsDef {
    #[serde(with = "file_types")]
    types: Vec<FileType>,
    initial_folder: Option<PathBuf>,
    suggested_name: Option<String>,
    confirm_label: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "DialogError")]
pub(crate) enum DialogErrorDef {
    Failed(String),
}

/// The engine's record of a notification, as the program built it.
#[derive(Serialize, Deserialize)]
#[serde(remote = "Notification")]
pub(crate) struct NotificationDef {
    id: String,
    title: String,
    body: Option<String>,
    #[serde(with = "option_image")]
    image: Option<Image>,
    #[serde(with = "notification_actions")]
    actions: Vec<NotificationAction>,
    silent: bool,
}

/// The engine's record of one of a notification's actions.
#[derive(Serialize, Deserialize)]
#[serde(remote = "NotificationAction")]
pub(crate) struct NotificationActionDef {
    id: String,
    label: String,
    reply_placeholder: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "NotificationError")]
pub(crate) enum NotificationErrorDef {
    Denied,
    Failed(String),
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Activation")]
pub(crate) struct ActivationDef {
    id: String,
    action: Option<String>,
    reply: Option<String>,
}
