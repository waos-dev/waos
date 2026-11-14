//! The world's Rust face, generated from the crate's copy of the repository's `wit/`, which
//! `cargo xtask sync-wit` makes and nothing edits: the types wasmtime makes of `wasdk:program`'s
//! world and the packages it imports, `wasdk:platform` and `wasdk:media`, which the engine's
//! state uses directly, and the `Host` traits each interface's `host` module implements.
//!
//! `with` keeps each resource over the engine's own value for it, so the code serving an interface
//! makes a handle by pushing that value into the store's table: a view is the state's
//! `ProgramView`, a key's reply its `KeyReply`, the compositor and a composition layer the
//! shell's compositor and layer, a window its `ProgramWindow`, a window's
//! configuration the record of what the program set, a screen the plain value the list holds, an
//! exit request the lifecycle's, a view's text input the shell's side of it, a typeface the face
//! handed out with its id, a permission the guarded capability it stands for, a name lookup's
//! answer the network's, the clipboard, an item listed, a copy's options, a source, a paste's
//! request, the user's copy request and the user's paste the clipboard's, a menu's place, a
//! tray icon and its menus the menus', sound's resources web-audio-api's own values where it
//! has one (a param, a buffer, a periodic wave, a listener, a context's options) and the audio
//! module's elsewhere: each kind of context, any context, each kind of node, any node, and the
//! platform's own inputs, a dialog's options and the dialogs the file dialogs', the opener the
//! shell's opener, a notification, the notifications and their replies and scheduling the
//! notifications', a command the provider's record, a machine, a started command, its pipes and
//! its terminal the provider's own, held by the machines', the players and a player the media
//! players', the media assets and an asset the media assets', the image codecs, an image and the
//! hardware codecs the image codecs', and the readers, a reader and a frame the video frames', the
//! system's hardware buffers and any buffer the program holds the hardware buffers', and the
//! cameras, a camera,
//! its outputs and a capture session the cameras', and the workers and a worker the workers'.
//! The interfaces another crate serves are that crate's generated types: WASI's clocks, `io` and
//! sockets are `wasmtime-wasi`'s, but for the name lookup's stream of 0.2, which the network's
//! code serves, its HTTP client `wasmtime-wasi-http`'s, and `wasi:webgpu` is
//! the WebGPU host's, `waos-webgpu-host`, so a view's canvas context is the host's own resource; a
//! notification's time is Preview 3's system clock's `instant`, which `wasmtime-wasi` serves; and
//! a machine's root is `wasi:filesystem@0.2`'s own `descriptor`, the files' bindings', which the
//! files serve.
//! Where the world holds two versions of a WASI package, Preview 2's for what Rust's `std`
//! imports and Preview 3's for the clocks, the sockets a program awaits and the entry, the
//! generated modules carry the version in their names (`cli0_3_0`), and a key of `with` names
//! the version it maps. A stream-returning function is given store access, which
//! a stream needs and an `async func` gets on its own.
//!
//! No derive is added to the generated types: `additional_derives` applies to every type of the
//! world, and a variant holding a resource cannot be `PartialEq`. A type the state compares whole
//! gets its `PartialEq` beside that comparison.

wasmtime::component::bindgen!({
    path: "wit",
    world: "wasdk:program/program",
    require_store_data_send: true,
    imports: {
        "wasdk:platform/views.[method]view.events": store | tracing | trappable,
        // Waits while the program is frozen, as a submission does; to the program a plain call.
        "wasdk:platform/views.[method]view.present": async | store | tracing | trappable,
        "wasdk:platform/views.view-changes": store | tracing | trappable,
        // Waits while the program is frozen, as a submission does; to the program a plain call.
        "wasdk:platform/views.[method]composition-layer.present": async | store | tracing | trappable,
        "wasdk:platform/screens.screen-changes": store | tracing | trappable,
        "wasdk:platform/system.changes": store | tracing | trappable,
        "wasdk:platform/lifecycle.state-changes": store | tracing | trappable,
        "wasdk:platform/lifecycle.exit-requests": store | tracing | trappable,
        "wasdk:platform/lifecycle.open-requests": store | tracing | trappable,
        "wasdk:platform/fonts.changes": store | tracing | trappable,
        "wasdk:platform/fonts.[method]typeface.open-stream": store | tracing | trappable,
        "wasdk:platform/permissions.[method]permission.request": store | tracing | trappable,
        "wasdk:platform/permissions.[method]permission.changes": store | tracing | trappable,
        "wasdk:platform/clipboard.[method]clipboard.changes": store | tracing | trappable,
        "wasdk:platform/clipboard.[method]source.requests": store | tracing | trappable,
        "wasdk:platform/clipboard.[method]request.provide": store | tracing | trappable,
        "wasdk:platform/menus.[method]menu-bar.chosen": store | tracing | trappable,
        "wasdk:platform/menus.[method]attached-menu.chosen": store | tracing | trappable,
        "wasdk:platform/menus.[method]tray-icon.events": store | tracing | trappable,
        "wasdk:platform/audio.[method]base-audio-context.state-changes": store | tracing | trappable,
        "wasdk:platform/notifications.[method]notifications.activations": store | tracing | trappable,
        "wasdk:platform/machines.[method]pipes.output": store | tracing | trappable,
        "wasdk:platform/machines.[method]pipes.errors": store | tracing | trappable,
        "wasdk:platform/machines.[method]terminal.output": store | tracing | trappable,
        "wasdk:media/media-players.[method]media-player.state-changes": store | tracing | trappable,
        "wasdk:media/caption-settings.[method]caption-settings.style-changes": store | tracing | trappable,
        "wasdk:media/media-players.[method]media-player.cue-changes": store | tracing | trappable,
        "wasdk:media/media-players.[method]media-player.metadata-changes": store | tracing | trappable,
        "wasdk:media/cameras.[method]cameras.changes": store | tracing | trappable,
        "wasdk:media/cameras.[method]capture-session.state-changes": store | tracing | trappable,
        "wasdk:media/cameras.[method]capture-session.video-rotation-angle-changes": store | tracing | trappable,
        "wasdk:platform/workers.[method]worker.messages": store | tracing | trappable,
        default: tracing | trappable,
    },
    trappable_error_type: {
        "wasi:sockets/network@0.2.12.error-code" => wasmtime_wasi::p2::SocketError,
    },
    with: {
        "wasdk:platform/views.view": crate::views::ProgramView,
        "wasdk:platform/views.text-input": crate::views::host::TextInput,
        "wasdk:platform/views.key-reply": crate::views::KeyReply,
        "wasdk:platform/views.compositor": crate::views::host::Compositor,
        "wasdk:platform/views.composition-layer": crate::views::host::CompositionLayer,
        "wasdk:platform/windows.window": crate::windows::ProgramWindow,
        "wasdk:platform/windows.window-config": crate::windows::WindowConfig,
        "wasdk:platform/windows.size-limits": crate::windows::SizeLimits,
        "wasdk:platform/screens.screen": crate::screens::Screen,
        "wasdk:platform/lifecycle.exit-request": crate::lifecycle::ExitRequest,
        "wasdk:platform/fonts.typeface": crate::fonts::Typeface,
        "wasdk:platform/permissions.permission": crate::permissions::Permission,
        "wasdk:platform/trust.trust": crate::trust::Trust,
        "wasdk:platform/clipboard.clipboard": crate::clipboard::PlatformClipboard,
        "wasdk:platform/clipboard.clipboard-item": crate::clipboard::ListedItem,
        "wasdk:platform/clipboard.copy-options": crate::clipboard::CopyOptions,
        "wasdk:platform/clipboard.source": crate::clipboard::Source,
        "wasdk:platform/clipboard.request": crate::clipboard::Request,
        "wasdk:platform/clipboard.copy-request": crate::clipboard::CopyRequest,
        "wasdk:platform/clipboard.paste": crate::clipboard::Paste,
        "wasdk:platform/menus.menu-bar": crate::menus::MenuBar,
        "wasdk:platform/menus.context-menus": crate::menus::ContextMenus,
        "wasdk:platform/menus.tray-icons": crate::menus::TrayIcons,
        "wasdk:platform/menus.tray-icon": crate::menus::TrayIcon,
        "wasdk:platform/menus.attached-menu": crate::menus::AttachedMenu,
        "wasdk:platform/menus.menu-at-icon": crate::menus::MenuAtIcon,
        "wasdk:platform/audio.audio": crate::audio::Audio,
        "wasdk:platform/audio.audio-context-options": waos_capabilities::web_audio_api::context::AudioContextOptions,
        "wasdk:platform/audio.base-audio-context": crate::audio::BaseContext,
        "wasdk:platform/audio.audio-context": crate::audio::Context,
        "wasdk:platform/audio.offline-audio-context": crate::audio::OfflineContext,
        "wasdk:platform/audio.audio-param": waos_capabilities::web_audio_api::AudioParam,
        "wasdk:platform/audio.audio-node": crate::audio::AnyNode,
        "wasdk:platform/audio.audio-buffer": waos_capabilities::web_audio_api::AudioBuffer,
        "wasdk:platform/audio.periodic-wave": waos_capabilities::web_audio_api::PeriodicWave,
        "wasdk:platform/audio.audio-listener": waos_capabilities::web_audio_api::AudioListener,
        "wasdk:platform/audio.audio-destination-node": crate::audio::handles::AudioDestinationNode,
        "wasdk:platform/audio.gain-node": crate::audio::handles::GainNode,
        "wasdk:platform/audio.oscillator-node": crate::audio::handles::OscillatorNode,
        "wasdk:platform/audio.audio-buffer-source-node": crate::audio::handles::AudioBufferSourceNode,
        "wasdk:platform/audio.constant-source-node": crate::audio::handles::ConstantSourceNode,
        "wasdk:platform/audio.delay-node": crate::audio::handles::DelayNode,
        "wasdk:platform/audio.biquad-filter-node": crate::audio::handles::BiquadFilterNode,
        "wasdk:platform/audio.iir-filter-node": crate::audio::handles::IirFilterNode,
        "wasdk:platform/audio.dynamics-compressor-node": crate::audio::handles::DynamicsCompressorNode,
        "wasdk:platform/audio.convolver-node": crate::audio::handles::ConvolverNode,
        "wasdk:platform/audio.wave-shaper-node": crate::audio::handles::WaveShaperNode,
        "wasdk:platform/audio.stereo-panner-node": crate::audio::handles::StereoPannerNode,
        "wasdk:platform/audio.panner-node": crate::audio::handles::PannerNode,
        "wasdk:platform/audio.analyser-node": crate::audio::handles::AnalyserNode,
        "wasdk:platform/audio.channel-splitter-node": crate::audio::handles::ChannelSplitterNode,
        "wasdk:platform/audio.channel-merger-node": crate::audio::handles::ChannelMergerNode,
        "wasdk:platform/audio.stream-input-node": crate::audio::StreamInput,
        "wasdk:platform/audio.microphone-node": crate::audio::Microphone,
        "wasdk:platform/file-dialogs.open-options": crate::file_dialogs::OpenOptions,
        "wasdk:platform/file-dialogs.folder-options": crate::file_dialogs::FolderOptions,
        "wasdk:platform/file-dialogs.save-options": crate::file_dialogs::SaveOptions,
        "wasdk:platform/file-dialogs.file-dialogs": crate::file_dialogs::FileDialogs,
        "wasdk:platform/opener.opener": crate::opener::Opener,
        "wasdk:platform/notifications.notification": crate::notifications::Notification,
        "wasdk:platform/notifications.notifications": crate::notifications::Notifications,
        "wasdk:platform/notifications.notification-replies": crate::notifications::NotificationReplies,
        "wasdk:platform/notifications.notification-scheduling": crate::notifications::NotificationScheduling,
        "wasdk:platform/machines.command": waos_capabilities::Command,
        "wasdk:platform/machines.child": crate::machines::Child,
        "wasdk:platform/machines.pipes": crate::machines::Pipes,
        "wasdk:platform/machines.terminal": crate::machines::Terminal,
        "wasdk:platform/machines.machine": crate::machines::Machine,
        "wasdk:platform/machines.machines": crate::machines::Machines,
        "wasdk:platform/hardware-buffers.hardware-buffer-allocator": crate::hardware_buffers::host::HardwareBufferAllocator,
        "wasdk:platform/hardware-buffers.hardware-buffer": crate::hardware_buffers::ProgramHardwareBuffer,
        "wasdk:media/video-frames.video-frame": crate::video_frames::ProgramVideoFrame,
        "wasdk:media/video-frames.video-frame-readers": crate::video_frames::VideoFrameReaders,
        "wasdk:media/video-frames.video-frame-reader": crate::video_frames::VideoFrameReader,
        "wasdk:media/media-players.media-players": crate::media_players::MediaPlayers,
        "wasdk:media/media-players.media-player": crate::media_players::MediaPlayer,
        "wasdk:media/media-assets.media-assets": crate::media_assets::MediaAssets,
        "wasdk:media/media-assets.media-asset": crate::media_assets::MediaAsset,
        "wasdk:media/image-codecs.image-codecs": crate::image_codecs::ImageCodecs,
        "wasdk:media/image-codecs.image-source": crate::image_codecs::ImageSource,
        "wasdk:media/image-codecs.hardware-image-codecs": crate::image_codecs::HardwareImageCodecs,
        "wasdk:media/caption-settings.caption-settings": crate::caption_settings::CaptionSettings,
        "wasdk:media/cameras.cameras": crate::cameras::Cameras,
        "wasdk:media/cameras.camera": crate::cameras::Camera,
        "wasdk:media/cameras.camera-outputs": crate::cameras::CameraOutputs,
        "wasdk:media/cameras.capture-session": crate::cameras::CaptureSession,
        "wasdk:platform/workers.workers": crate::workers::Workers,
        "wasdk:platform/workers.worker": crate::workers::Worker,
        "wasi:filesystem/types@0.2.12": crate::files::p2::bindings::wasi::filesystem::types,
        "wasi:clocks/wall-clock@0.2.12": wasmtime_wasi::p2::bindings::clocks::wall_clock,
        "wasi:webgpu/webgpu": waos_webgpu_host::wasi::webgpu::webgpu,
        "wasi:clocks/types@0.3.0": wasmtime_wasi::p3::bindings::clocks::types,
        "wasi:clocks/monotonic-clock@0.3.0": wasmtime_wasi::p3::bindings::clocks::monotonic_clock,
        "wasi:clocks/system-clock@0.3.0": wasmtime_wasi::p3::bindings::clocks::system_clock,
        "wasi:clocks/monotonic-clock@0.2.12": wasmtime_wasi::p2::bindings::clocks::monotonic_clock,
        "wasi:io": wasmtime_wasi_io::bindings::wasi::io,
        "wasi:sockets/network@0.2.12": wasmtime_wasi::p2::bindings::sockets::network,
        "wasi:sockets/instance-network@0.2.12": wasmtime_wasi::p2::bindings::sockets::instance_network,
        "wasi:sockets/tcp@0.2.12": wasmtime_wasi::p2::bindings::sockets::tcp,
        "wasi:sockets/tcp-create-socket@0.2.12": wasmtime_wasi::p2::bindings::sockets::tcp_create_socket,
        "wasi:sockets/udp@0.2.12": wasmtime_wasi::p2::bindings::sockets::udp,
        "wasi:sockets/udp-create-socket@0.2.12": wasmtime_wasi::p2::bindings::sockets::udp_create_socket,
        "wasi:sockets/ip-name-lookup@0.2.12.resolve-address-stream": crate::network::host::ResolveAddressStream,
        "wasi:sockets/types@0.3.0": wasmtime_wasi::p3::bindings::sockets::types,
        "wasi:http/types@0.3.0": wasmtime_wasi_http::p3::bindings::http::types,
        "wasi:http/client@0.3.0": wasmtime_wasi_http::p3::bindings::http::client,
    },
});
