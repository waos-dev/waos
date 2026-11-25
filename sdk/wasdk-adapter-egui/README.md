# wasdk-adapter-egui

egui on WaOS: what `egui-winit` is on a winit window, for a view of WaOS.

- **Takes:** a view's events (`wasdk::views::ViewEvent`) and an `egui::Context`.
- **Provides:** `State`, which turns those events into egui's input for a pass (`take_egui_input`) and carries out what egui asks of the system after it (`handle_platform_output`): the cursor's shape, a copy, text input starting and stopping, a link to open.
- **Who uses it:** an app whose UI is egui. Drawing is not here: `egui-wgpu` draws on the wgpu device `wasdk-wgpu` gives the app.

Its names are `egui-winit`'s, so an app written for winit swaps one `State` for the other. Two things differ, because a view's events are not plain data:

- **An event is handed in and handed back.** A key comes with the reply the system waits on, and a copy or a paste with what carries it out. `on_view_event` takes the event, and answers it back unless egui used it up, for the app to act on.
- **A paste arrives a moment later,** as its own input, since WaOS reads the clipboard without blocking.

egui's types are in this interface, so the crate goes with one version of egui: the workspace's.
