//! The program's one turn loop: every event of every stream, and the framework's own wake-ups,
//! as one push each into Waft's client, and a refresh's videos' new frames, which the views draw
//! themselves. A refresh's frames, which arrive on the stream of each
//! view it serves, are one frame of the client's for them all, once every one has arrived
//! (`refreshes`).
//!
//! The invariants: the platform is brought up to date before the client hears of a change, so the
//! client reads the new value at once (Waft's `EmbedderClient`); and each push returns before the
//! next begins, so the client gets one refresh at a time, never one inside another call.

use std::ops::ControlFlow;
use std::pin::pin;
use std::rc::Rc;
use std::time::Duration;

use futures::future::{self, Either};
use waft_embedder::{
    ClipboardEvent, EditCommand, EmbedderClient, Frame, FrameVsync, Instant, KeyData, Platform,
    RequestId, View, ViewId,
};
use waft_shell::Shell;
use wasdk::monotonic_clock;
use wasdk::screens::ScreenChange;
use wasdk::system::SystemChange;
use wasdk::views::{self, KeyInput, TextEditingValue, ViewChange, ViewEvent};

use crate::app_exit::{quit_without_views, reply_to};
use crate::clipboard::{copy_request, pasted_items};
use crate::dispatcher::Alarm;
use crate::events::{Event, Events, Notice};
use crate::focus::framework_focus_event;
use crate::keys::key_data;
use crate::lifecycle_states::framework_lifecycle_state;
use crate::notifications::framework_activation;
use crate::open_requests::framework_open_request;
use crate::platform::WasdkPlatform;
use crate::pointer::pointer_report;
use crate::refreshes::{Refresh, Refreshes};
use crate::text_editing::{framework_action, framework_editing_value};
use crate::typing::ChordModifiers;
use crate::windows::view_error;

pub(crate) struct EventLoop {
    platform: Rc<WasdkPlatform>,
    client: Shell,
    events: Events,
    alarm: Alarm,
    /// Whether Control or Command is held, which keeps a key from typing.
    chord_modifiers: ChordModifiers,
    /// The refreshes whose frames have arrived on some of their views' streams.
    refreshes: Refreshes,
}

/// What the loop turns on.
enum Turn {
    Event(Event),
    /// The framework's timer wakeup arrived before any event.
    TimerWakeup,
    /// Every stream has run out; there is nothing left to turn on.
    Ended,
}

impl EventLoop {
    /// The loop that pushes `events` into `client`, over `platform`.
    pub(crate) fn new(platform: Rc<WasdkPlatform>, client: Shell, events: Events) -> EventLoop {
        EventLoop {
            platform,
            client,
            events,
            alarm: Alarm::default(),
            chord_modifiers: ChordModifiers::default(),
            refreshes: Refreshes::default(),
        }
    }

    /// Turns until the app quits.
    pub(crate) async fn run(&mut self) {
        loop {
            self.alarm.set(self.platform.wake().timer_wakeup());
            let carry_on = match self.next_turn().await {
                Turn::Ended => ControlFlow::Break(()),
                Turn::TimerWakeup => {
                    self.timer_arrived();
                    ControlFlow::Continue(())
                }
                Turn::Event(event) => self.deliver(event),
            };
            if carry_on.is_break() {
                return;
            }
            // TODO: remove once wasmtime hands a program its ready events in the order they became
            // ready. wasmtime 50 hands over the ready stream created first, so while each frame takes
            // longer than a refresh the view's next frame is always ready and wins every pick, and a
            // stream opened later, a player's state among them, gets nothing until frames catch up.
            // While this yields the stream just read is not read again, so another ready stream's
            // event goes first: one each frame, at 1 to 2 µs a yield
            // (`experiments/2026-11-09-stream-starvation-release`).
            wasdk::yield_async().await;
        }
    }

    /// The next event from any stream, or the alarm ringing first.
    async fn next_turn(&mut self) -> Turn {
        let events = &mut self.events;
        let alarm = &mut self.alarm;
        match future::select(pin!(events.next()), pin!(alarm.ring())).await {
            Either::Left((Some(event), _)) => Turn::Event(event),
            Either::Left((None, _)) => Turn::Ended,
            Either::Right(((), _)) => Turn::TimerWakeup,
        }
    }

    /// One event as one push into the client.
    fn deliver(&mut self, event: Event) -> ControlFlow<()> {
        match event {
            Event::View(id, event) => self.view_event(id, event),
            Event::ViewChange(change) => self.view_change(change),
            Event::ScreenChange(change) => self.screens_changed(change),
            Event::SystemChange(change) => self.system_changed(change),
            Event::LifecycleState(state) => self
                .client
                .app_lifecycle_state_changed(framework_lifecycle_state(state)),
            Event::ExitRequest(request) => self.client.request_app_exit(reply_to(request)),
            Event::FontsChanged => self.client.system_fonts_changed(),
            Event::ClipboardChanged => self.client.clipboard_changed(),
            Event::MenuSelected(id) => self.client.menu_selected(id),
            Event::OpenRequested(request) => {
                self.client.open_requested(framework_open_request(request))
            }
            Event::NotificationActivated(activation) => self
                .client
                .notification_activated(framework_activation(activation)),
            Event::TrayIcon(icon, event) => self.client.tray_icon_event(icon, event),
            Event::Notice(Notice::WakeNow) => self.wake_now(),
            Event::Notice(Notice::Quit) => return ControlFlow::Break(()),
        }
        ControlFlow::Continue(())
    }

    fn view_event(&mut self, id: ViewId, event: ViewEvent) {
        match event {
            ViewEvent::Frame(frame) => self.frame_arrived(id, frame),
            ViewEvent::Metrics(metrics) => {
                self.platform.set_metrics(id, metrics);
                self.client.view_metrics_changed(id);
            }
            ViewEvent::Pointer(pointer) => self
                .client
                .pointer_reports(vec![pointer_report(id, &pointer)]),
            ViewEvent::Key(key) => self.key_arrived(id, key),
            ViewEvent::Focus(focus) => self
                .client
                .view_focus_changed(framework_focus_event(id, focus)),
            ViewEvent::TextInputEditingValue(value) => self.text_input_edited(id, value),
            ViewEvent::TextInputAction(action) => {
                self.client.text_input_action(id, framework_action(action))
            }
            ViewEvent::TextInputClosed => self.text_input_closed(id),
            ViewEvent::CloseRequested => self.client.view_close_requested(id),
            ViewEvent::Copy(request) => self
                .client
                .clipboard_event(id, ClipboardEvent::Copy(copy_request(request))),
            ViewEvent::Cut(request) => self
                .client
                .clipboard_event(id, ClipboardEvent::Cut(copy_request(request))),
            ViewEvent::Paste(paste) => self
                .client
                .clipboard_event(id, ClipboardEvent::Paste(pasted_items(paste))),
            ViewEvent::Edit(command) => self.client.edit_command(id, edit_command(command)),
            ViewEvent::PopRoute => self.pop_route(id),
        }
    }

    /// The user asked to go back, and the app said it handles that: its navigators go back. What
    /// the client answers was said in advance, through `set-handles-back`
    /// (`Platform::set_framework_handles_back`), so nobody waits for it.
    fn pop_route(&mut self, view: ViewId) {
        let _went_back = self.client.pop_route(view);
    }

    /// The frame asked of the view `id` arrived: once the frames of every view its refresh
    /// serves have, the client draws that refresh. One still on the view's stream when the view
    /// went is dropped, as Waft's winit host drops a frame asked of a view that is gone.
    fn frame_arrived(&mut self, id: ViewId, frame: views::Frame) {
        if !self.platform.has_view(id) {
            return;
        }
        let platform = &self.platform;
        let complete = self
            .refreshes
            .frame_arrived(id, &frame, |view| platform.has_view(view));
        if let Some(refresh) = complete {
            self.draw(refresh);
        }
    }

    /// Draws `refresh`: each view it serves is told when its picture reaches the screen, the client
    /// draws one frame of those the framework asked a frame of, and each view showing a video takes
    /// its new frames, as Waft's own hosts take a video's frames at each vsync.
    fn draw(&mut self, refresh: Refresh) {
        let views: Vec<_> = refresh
            .views
            .iter()
            .filter_map(|&id| self.platform.wasdk_view(id))
            .collect();
        for view in &views {
            view.set_display_time(refresh.display_time);
        }
        let asked: Vec<ViewId> = views
            .iter()
            .filter(|view| view.take_framework_frame())
            .map(|view| view.id())
            .collect();
        if !asked.is_empty() {
            // The refresh's start and the time its picture is expected on screen are the vsync's
            // start and target, as Flutter's embedder API takes them (`FlutterEngineOnVsync`).
            let frame = Frame {
                elapsed: elapsed(),
                vsync: Some(FrameVsync {
                    start: refresh.began,
                    target: Duration::from_nanos(refresh.display_time),
                }),
            };
            self.client.frame(&asked, frame);
        }
        for view in &views {
            view.draw_new_video_frames();
        }
    }

    fn view_change(&mut self, change: ViewChange) {
        match change {
            ViewChange::Added(view) => {
                if let Some(view) = self.platform.add_view(view) {
                    self.events.add_view(&view);
                    self.client.view_added(view.id());
                }
            }
            ViewChange::Removed(id) => self.view_removed(ViewId(id)),
            ViewChange::RequestFailed(failure) => {
                let request = RequestId(failure.request);
                self.platform.request_failed(request);
                self.client
                    .view_request_failed(request, view_error(failure.error));
            }
        }
    }

    /// The program no longer has the view `id`: its window was closed, or the shell took it
    /// away, and a refresh that waited for its frame is drawn without it; with no view left, the
    /// app quits (`app_exit`).
    fn view_removed(&mut self, id: ViewId) {
        if !self.platform.remove_view(id) {
            return;
        }
        self.client.view_removed(id);
        for refresh in self.refreshes.view_removed(id) {
            self.draw(refresh);
        }
        if let Some(exit) = self.platform.app_exit() {
            quit_without_views(exit, self.platform.views().len());
        }
    }

    /// Every change of the screens reads them all again, so the change itself says nothing more.
    fn screens_changed(&mut self, _change: ScreenChange) {
        self.platform.refresh_screens();
        self.client.screens_changed();
    }

    fn system_changed(&mut self, change: SystemChange) {
        match change {
            SystemChange::PlatformBrightness(brightness) => {
                self.platform.set_brightness(brightness);
                self.client.platform_brightness_changed();
            }
            SystemChange::Locales(locales) => {
                self.platform.set_locales(locales);
                self.client.locales_changed();
            }
            // Waft keeps no time zone, as Flutter's engine keeps none: an app reads
            // `system::time_zone` and `utc_offset` itself, and listens for this change itself.
            SystemChange::TimeZone(_) => {}
        }
    }

    /// A key goes to the framework, and one it did not handle is typed into the view's text
    /// input: Flutter's keyboard managers hand the text input plugin only what the framework
    /// declined. The key's reply says whether either took it.
    fn key_arrived(&mut self, id: ViewId, key: KeyInput) {
        let KeyInput { event, reply } = key;
        let key = key_data(event);
        self.chord_modifiers.record(&key);
        let handled = self.client.key_data(key.clone()) || self.type_into_text_input(id, &key);
        reply.handled(handled);
    }

    /// Types `key` into the view's text input; answers whether it edited the text or acted.
    fn type_into_text_input(&mut self, id: ViewId, key: &KeyData) -> bool {
        let Some(text_input) = self.platform.text_input(id) else {
            return false;
        };
        let typed = text_input.type_key(key, self.chord_modifiers.any_held());
        let took = typed.edited.is_some() || typed.action.is_some();
        if let Some(value) = typed.edited {
            self.client.text_input_editing_value(id, value);
        }
        if let Some(action) = typed.action {
            self.client.text_input_action(id, action);
        }
        took
    }

    /// An input method's edit: kept for the keys typed after it, and handed to the framework,
    /// which does not echo it back.
    fn text_input_edited(&mut self, id: ViewId, value: TextEditingValue) {
        let value = framework_editing_value(value);
        if let Some(text_input) = self.platform.text_input(id) {
            text_input.platform_edited(&value);
        }
        self.client.text_input_editing_value(id, value);
    }

    fn text_input_closed(&mut self, id: ViewId) {
        if let Some(text_input) = self.platform.text_input(id) {
            text_input.platform_ended_editing();
        }
        self.client.text_input_closed(id);
    }

    fn timer_arrived(&mut self) {
        self.platform.wake().timer_arrived(Instant::now());
        self.client.wake(elapsed());
    }

    fn wake_now(&mut self) {
        // Taken before the wake, so a task made ready inside it asks for another.
        self.platform.wake().take_wake_now();
        self.client.wake(elapsed());
    }
}

/// One of Edit's commands, as Waft names it.
fn edit_command(command: views::EditCommand) -> EditCommand {
    match command {
        views::EditCommand::Undo => EditCommand::Undo,
        views::EditCommand::Redo => EditCommand::Redo,
        views::EditCommand::SelectAll => EditCommand::SelectAll,
        views::EditCommand::Delete => EditCommand::Delete,
    }
}

/// Time on the program's monotonic clock, the one every frame and event is stamped on.
fn elapsed() -> Duration {
    Duration::from_nanos(monotonic_clock::now())
}
