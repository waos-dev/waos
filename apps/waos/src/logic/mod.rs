//! Plain Rust, tested without Waft's UI: WaOS's command line and where it shows its library
//! (`command_line`); where the app keeps its files, the programs running and the dev server that
//! installs what `waos install` sends and uninstalls (`dev_install`), launches and stops a program
//! (`dev_launch`), attaches to what a launch writes (`launch_outputs`, `dev_attach`), or opens the
//! files it sends with a program (`opening`), and `waos open`'s side of that (`open_command`), or
//! answers its screenshot of a program's windows (`dev_screenshot`), gives them its input
//! (`dev_input`) or sizes one (`dev_resize`), or reads and sets a program's settings
//! (`dev_settings`), with what WaOS asks each program's user now, which an attach says and a
//! screenshot and an input read (`asking_now`), its file dialogs among it
//! (`noted_file_dialogs`); what each program
//! runs with (`environments`): this computer's capabilities and the grants of the store's settings;
//! the library's side of the store: a package opened with WaOS, what a source's listing offers
//! against what is installed, what runs, at most one instance of each app, and every way of
//! opening one (`app_instances`), and WaOS's one startup, which puts the domains together over where
//! each keeps what it owns (`stores`) for the desktop app, the headless entry and the tests
//! (`startup`), each installed program's shortcut on Android's home screen and WaOS's
//! icon's menu, made to match what is installed (`program_shortcuts`), and a settings
//! page and what the user changes on it (`setting_changes`); when the desktop's launcher shows and
//! goes (`launcher_rules`), and where it shows (`launcher_placement`); where the programs' prompts go (`prompts`), their questions
//! and the picker of their sound's output among them, with a program's name as its user knows it
//! (`program_name`), and whether WaOS may reach the phone's files, which a pick of This computer
//! asks for on Android (`phone_files`), and where the engine's indicator of who records goes (`recording_indicator`);
//! their clipboard calls (`clipboard_calls`), with what goes between their clipboard and Waft's
//! (`clipboard`); their menu calls (`menu_calls`), and WaOS's own menu bar (`menu_bar`); their
//! notifications (`notification_calls`), as the system shows them
//! (`system_notifications`); their players' state and
//! metadata (`player_states`) and its cues (`cues`) as the program reads them, and their frame
//! readers, Waft's once a player is given one (`frame_readers`); an image's values as the program
//! reads them (`image_values`), a
//! camera's values as the program reads them (`camera_values`); the machine's facts as Waft reports
//! them, in the engine's terms; where a program's view request goes, and the screens' refreshes
//! over Waft's vsync waiter, from whichever thread the engine calls them on, the refusal of every
//! request of a program in a tab of the deck (`program_places`), the box a tab's page takes
//! (`tab_page`), the deck's tabs as a list, with the rules of what a tab is (`tab_list`), and where the deck draws
//! each tab, on its strip of pages, in its overview's grid and between them (`deck_geometry`); whether
//! a quit may go ahead; and, on macOS, the device numbers of the pointer reports a prompt's dialog in a program's
//! app sends WaOS (`forwarded_pointers`). The rest of a program's window is
//! `waos-program-window`'s.
//!
//! The invariant: nothing here names Waft's application or a widget. What ties this to the
//! application is `state`'s.

pub(crate) mod activity;
pub(crate) mod app_instances;
pub(crate) mod app_store;
pub(crate) mod asking_now;
pub(crate) mod camera_values;
pub(crate) mod caption_style;
pub(crate) mod clipboard;
pub(crate) mod clipboard_calls;
pub(crate) mod command_line;
pub(crate) mod cues;
pub(crate) mod deck_geometry;
#[cfg(desktop)]
pub(crate) mod dev_announce;
#[cfg(desktop)]
pub(crate) mod dev_attach;
#[cfg(desktop)]
pub(crate) mod dev_extensions;
#[cfg(desktop)]
pub(crate) mod dev_input;
#[cfg(desktop)]
pub(crate) mod dev_inspect;
#[cfg(desktop)]
pub(crate) mod dev_install;
#[cfg(desktop)]
pub(crate) mod dev_launch;
#[cfg(desktop)]
pub(crate) mod dev_network;
#[cfg(desktop)]
pub(crate) mod dev_pairing;
#[cfg(desktop)]
pub(crate) mod dev_refusal;
#[cfg(desktop)]
pub(crate) mod dev_resize;
#[cfg(desktop)]
pub(crate) mod dev_screenshot;
#[cfg(desktop)]
pub(crate) mod dev_server;
#[cfg(desktop)]
pub(crate) mod dev_settings;
#[cfg(desktop)]
pub(crate) mod developer_access;
pub(crate) mod environments;
pub(crate) mod file_dialog_calls;
pub(crate) mod folders;
#[cfg(target_os = "macos")]
pub(crate) mod forwarded_pointers;
pub(crate) mod frame_readers;
pub(crate) mod hardware_buffers;
pub(crate) mod held_starts;
pub(crate) mod icon_rows;
pub(crate) mod image_values;
pub(crate) mod inspected_programs;
pub(crate) mod launch_outputs;
#[cfg(desktop)]
pub(crate) mod launcher_placement;
#[cfg(desktop)]
pub(crate) mod launcher_rules;
pub(crate) mod library_layout;
pub(crate) mod machine_facts;
#[cfg(target_os = "macos")]
pub(crate) mod macos_shell;
pub(crate) mod menu_bar;
pub(crate) mod menu_calls;
#[cfg(desktop)]
pub(crate) mod network_addresses;
pub(crate) mod noted_file_dialogs;
pub(crate) mod notification_calls;
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "read on every system, written where a program has an app of its own"
    )
)]
pub(crate) mod notification_permissions;
pub(crate) mod offers;
pub(crate) mod often_used;
#[cfg(desktop)]
pub(crate) mod open_command;
pub(crate) mod open_video_decoders;
pub(crate) mod opened_package;
pub(crate) mod opening;
pub(crate) mod package_copies;
pub(crate) mod permissions;
pub(crate) mod phone_files;
pub(crate) mod placement;
pub(crate) mod player_states;
pub(crate) mod preferences;
#[cfg(any(windows, target_os = "linux", test))]
#[cfg_attr(
    not(any(windows, target_os = "linux")),
    allow(
        dead_code,
        reason = "tested on every desktop, kept on Windows and Linux"
    )
)]
pub(crate) mod program_entries;
pub(crate) mod program_faces;
pub(crate) mod program_identities;
pub(crate) mod program_name;
pub(crate) mod program_places;
pub(crate) mod program_shortcuts;
pub(crate) mod prompts;
#[cfg(desktop)]
pub(crate) mod quit_question;
pub(crate) mod recording_indicator;
pub(crate) mod setting_changes;
#[cfg(any(target_os = "macos", target_os = "android"))]
pub(crate) mod simulated_cameras;
pub(crate) mod startup;
pub(crate) mod stores;
pub(crate) mod system_notifications;
pub(crate) mod tab_list;
pub(crate) mod tab_page;
#[cfg(test)]
pub(crate) mod test_engine;
#[cfg(test)]
pub(crate) mod test_installs;
#[cfg(test)]
pub(crate) mod test_outputs;
#[cfg(any(target_os = "macos", target_os = "android"))]
pub(crate) mod test_pattern;
#[cfg(desktop)]
pub(crate) mod tray_image;
pub(crate) mod video_decoder_calls;
pub(crate) mod video_decoder_thread;
pub(crate) mod video_values;
pub(crate) mod vsync_waiter;
