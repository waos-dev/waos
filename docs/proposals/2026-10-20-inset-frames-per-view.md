# Inset: a frame per view

**Decisions taken (2026-10-20).** The lead took these, one question at a time:

1. **Each view gets its own frame, at its own screen's refresh**, instead of one frame for the whole app paced by the first window's screen. It settles a better foundation for native Inset and for programs in WaOS alike: WaOS gives frames per view, and a whole-app frame paced by one view stalls every window when that view is hidden. *Refined by decision 13: the views on one screen share its refresh.*
2. **A frame request names its target** (`FrameTarget`): one view, every view, or any view. Nearly all frame work already knows its view through its context; every view is for fonts changing, the first frame and coming back from the background; any view is for building dirty elements and for restoration. *Revised by decision 10: no request names "any view".*
3. **The target is an argument on Flutter's existing scheduling calls**, so every call site must choose one and the compiler finds them all.
4. **A ticker takes its view from its provider's context** and follows a move to another view through `did_change_dependencies`, beside the `TickerMode` update the mixins already do; Compose and SwiftUI take an animation's clock from its context the same way. *Revised by decision 12: it looks its view up each time.*
5. **A hidden window gets no frames**: a request is held while its window is minimized or occluded and served at the first refresh once it shows, as the web pauses `requestAnimationFrame` in a hidden tab.
6. **Build and layout stay shared and incremental; tickers, paint and composite are per view** (option II of four). Research found that building only a view's own subtree at its frame loses a `GlobalKey` subtree's state when it moves between views, and leaves render objects in other views never laid out until their own frame. Keeping Flutter's one build pass and one layout pass keeps both of Flutter's invariants. Views as separate trees, as in Compose and SwiftUI, were considered and left for later, for less change now.
7. **Each view paints only at its own frame**, not recursing into views anchored in it. *Extended by decision 11: it presents only when it painted something.*
8. **`WidgetTester::pump` gives a frame to every view that asked, and `pump_view` to one view**, so existing tests are unchanged.

**Decisions taken (2026-10-22)**, after checking how far Flutter has settled frames with several views:

9. **A frame belongs to a view, not to the app.** Flutter's one frame for the whole app is an intermediate state: its multiple-views design accepts one refresh rate for every view as a drawback, its open issue #146249 aims at a vsync per display that renders only that display's views, and in WaOS, where a hidden view gets no frames, a whole-app frame paced by a view that gets hidden stops the app. The platforms with separate windows give each window its frame: the web renders a document at its own rendering opportunities and a hidden one not at all, GTK gives a widget its toplevel's frame clock, and Android has each window ask for its own traversal.
10. **Every frame request names one view or every view; none names "any view".** An element asks for the frame of the view it is mounted in; an element above every `View` asks every view, since every view is below it, as Flutter marks every view for a cause that belongs to none. A post-frame callback names its view, or none: then it runs after the next frame of whichever view, which is right because every frame builds and lays out the whole shared tree; tying it to every view would hold it while one is hidden.
11. **A view presents at its frame only when it painted something or was marked**: Flutter's #192128, ported as it is. A view whose render tree had nothing to paint keeps showing its last frame; a new view, the warm-up frame and resuming mark views to present.
12. **A ticker looks its view up from its provider's context each time**: when it schedules a tick, to ask for that view's frame, and at each frame, to run only at its view's. A State need not tell the mixin that its element moved, which Flutter's mixins never ask, and a widget moved into another view, or under a `View` given another window, ticks at the new view's frames even while the old view is hidden. The lookup is one read of the element's map of inherited widgets, never a walk up the tree; Compose's `withFrameNanos` likewise reads its clock from its context at every call. *Replaced by decision 16.*
13. **A frame is a screen's refresh, for every view on that screen that asked**: `EmbedderClient::frame(views, frame)`. Vsync comes from a screen on every platform, a view only asks and is served while in sight; where something lives outside any one window, Android's `Choreographer` runs one `doFrame` per vsync for every window of the thread and the web one "update the rendering" for every document due, and Flutter's #146249 aims at rendering a display's views at its vsync. Callbacks that belong to no view then run once per refresh because the host says what a refresh is, not by a guess from time stamps, and views on one screen share one build.
14. **The frame's persistent callbacks are told the refresh's views**, as an argument, rather than reading which view the frame is for from a getter; they are the only callbacks that run at every frame, so the only ones that need it.
15. **WaOS stays per view for now**: its frame events arrive on each view's stream, so `inset-embedder-wasdk` hands each to Inset as a refresh of that one view, and an animation above every window ticks once per window per refresh there. WaOS gets the correct design, a program's views on one screen served by one refresh, as a step of its own.
16. **A ticker follows its widget to another view in the ticker mixins' `activate`**, where Flutter's mixins already re-read `TickerMode` for a new ancestor: the move tells the ticker its new view, and a pending tick moves with it and asks for that view's frame. A ticker's view changes only when a `GlobalKey` moves its State, which deactivates and then activates it, since a view's element is keyed by its view; so the scheduler keeps a fixed view per callback and asks nothing at each frame. Decision 12 was taken on the claim that `activate` missed a `View` given another window in place, which is wrong; GTK likewise moves a widget's tick callbacks to the new window's frame clock when it is realized there.

Flutter, as of its master on 2026-10-14, keeps one frame for all views: one `Animator` per engine; on macOS a display link per view, of which `onVSync` takes any one (`TODO`, issue #142845); Windows paced by the compositor's global rate and Linux by a fixed 60 Hz timer. Its design docs choose one widget tree with one refresh rate and one frame pipeline carrying every view, and leave per-window lifecycle to the window controllers. Presenting only the views that changed (#164320, as #179874 and #192128) has landed and been reverted twice for benchmark and package compatibility; its third landing waits on flutter/packages#13089. A vsync per display (#146249) is open with no design. This design goes past Flutter on purpose in the scheduler; each divergence is recorded in Inset's `PORTING.md`.

## The design

```text
host                                  Inset                                         app
  each screen: its display link       Platform::request_frame(view)  ◀── scheduler: one view or every view
  window hidden → request held        EmbedderClient::frame(views, frame) ──▶ frame(views of one screen):
  shown → served at next refresh                                             their tickers, and those of no view
                                                                             build: every dirty element
                                                                             layout: every dirty owner
                                                                             paint each; present each that painted or was marked
                                                                             finalize_tree, post-frame
```

### The embedder layer

```rust
pub trait Platform {
    /// Asks for one frame of `view`, at the next refresh of the screen it is on, while its window
    /// is in sight: Flutter's `PlatformDispatcher.scheduleFrame`, for one view.
    fn request_frame(&self, view: ViewId);
}

pub trait EmbedderClient {
    /// A screen refreshed at `frame.elapsed`; `views` are the views on it that asked for a frame
    /// and are in sight.
    fn frame(&mut self, views: &[ViewId], frame: Frame);
}
```

### The scheduler

```rust
/// The views a frame is asked of.
pub enum FrameTarget {
    /// One view: work inside that view's subtree.
    View(ViewId),
    /// Each view, at its own frame: the first frame, back from the background, an element above every view.
    EveryView,
}

impl SchedulerBinding {
    pub fn schedule_frame(app: &mut App, target: FrameTarget);
    pub fn ensure_visual_update(app: &mut App, target: FrameTarget);
    pub fn schedule_forced_frame(app: &mut App, target: FrameTarget);
    /// Runs `callback` at the next refresh of `view`, or of any view for `None`.
    pub fn schedule_frame_callback(app: &mut App, callback: FrameCallback, view: Option<ViewId>,
                                   rescheduling: bool, schedule_new_frame: bool) -> i64;
    /// `None`: after the next frame of whichever view; asks for no frame.
    pub fn add_post_frame_callback(app: &mut App, callback: FrameCallback, view: Option<ViewId>);
    /// The callback is told the views of each frame it runs at.
    pub fn add_persistent_frame_callback(app: &mut App, callback: PersistentFrameCallback);
    pub fn handle_begin_frame(app: &mut App, views: &[ViewId], time: Option<Duration>);
    pub fn handle_draw_frame(app: &mut App, views: &[ViewId]);
}

impl Ticker {
    /// Its provider passes `View::maybe_id_of(context)`.
    pub fn new(app: &mut App, on_tick: TickerCallback, view: Option<ViewId>) -> Handle<Ticker>;
    /// Its widget moved to `view`: a pending tick moves with it and asks for that view's frame.
    pub fn set_view(self: Handle<Self>, app: &mut App, view: Option<ViewId>);
}
```

### What requests what, and what a frame runs

```text
render object marked in view V         → V's PipelineOwner → a frame of V
element marked dirty in view V         → a frame of V        // BuildOwner asks once per view between build passes
element above every View marked dirty  → every view
ticker of a widget in V                → a frame of V        // V set when it is made, and in activate when its widget moves
ticker in a State above every View     → every view; it ticks once per refresh
the first frame, resuming              → every view, each marked to present
a new view                             → a frame of it, marked to present

frame(views, time):                    // one screen's refresh
  the tickers of those views, and those of no view, each once; the time never goes backwards
  build every dirty element in one pass
  flush_layout and flush_compositing_bits on the root owner (every owner)
  for each view: flush_paint on its PipelineOwner only, not recursing into anchored views
      → it had something to paint → it is marked to present
  present each view that is marked; the others keep showing their last frame
  finalize_tree
  post-frame callbacks for those views, then those for no view
```

The views of a refresh are fixed when it starts: a view that asks during it gets the next refresh, as with Android's `Choreographer` and the web.

### The hosts

```text
winit:   one call per vsync of a screen (one display link per screen), with the views on it that asked
         window minimized or occluded → the request is held; served at the first refresh once in sight
         a resize frame is a refresh of that one view
web:     requestAnimationFrame each refresh → its one view, if it asked (a hidden tab pauses it)
android: its one view
tests:   WidgetTester::pump(duration) → one refresh of every view that asked; pump_view(view, duration) → a refresh of that view
```

### WaOS

`inset-embedder-wasdk` maps `Platform::request_frame(view)` to `view.request-frame()`, and each view's frame event to `EmbedderClient::frame(&[view], frame)`: a refresh of that one view, until WaOS serves a program's views on one screen by one refresh (decision 15).

## The work, in order

Each step builds and passes on its own; the Inset steps stay uncommitted in `../inset-rs` for the lead's review.

1. Inset's embedder layer: `request_frame(view)` and `frame(view, …)`; each host delivers per view (winit per screen with hidden windows held, web, Android, the test platform), while the framework still runs its whole-app frame on any view's frame.
2. Inset's scheduler: `FrameTarget` on every scheduling call, each call site choosing; requests and frame phases per view; `Ticker`'s target, and the providers passing their view.
3. Inset's rendering: a view's frame paints and composites only that view; each view's `PipelineOwner` asks for its own view's frame.
4. Inset's tests: `pump` and `pump_view`; tests of a hidden view staying idle, two views at different rates, a `GlobalKey` move between views keeping its state, and an anchored view painting only at its own frame.
5. WaOS's embedder: request and frame per view, one to one; the gallery's test stays green, and gains a test with two windows, one hidden.

The revision of 2026-10-22, over those steps:

6. Port #192128's present rule: a view's `needs_composite_frame`, set from its `PipelineOwner`'s paint and by a new view, the warm-up frame and resuming.
7. Requests name a view: `FrameTarget` loses "any view"; `BuildOwner` asks per view, and an element above every `View` asks every view; `schedule_frame_callback` and `Ticker::new` take a way to find their view, asked when scheduling and at each frame; the mixins' view-following in `did_change_dependencies` goes; every call site passes its view.
8. Post-frame callbacks take an optional view; the image cache, restoration, the mouse tracker and the layer debug check pass none.
9. Tests: a dirty element in one view asks only that view; an element above the views asks every view; a post-frame callback with no view runs while another view is hidden; a view that painted nothing presents nothing.
10. WaOS: the gallery and `examples/two-windows` built and run against it.

After the lead's review of that revision (decisions 13–15), each step building and passing on its own:

11. One call per screen refresh: the embedder trait, the scheduler, the bindings and the persistent callbacks; every host, `inset-embedder-wasdk` included; `pump` and `pump_view`; the scheduler's guess of a refresh from time stamps, and its extra clocks, go.
12. The scheduler's request path in one place, and a refresh's callbacks taken as two lists, its views' and those of no view.
13. `BuildOwner` hands a dirty element to the binding, which picks the frame; callers pass a closure for a ticker's view.
14. winit: screens named by `ScreenId`; the frame scheduler follows window events only.
15. Tests: a named test view, test platforms that have it, shared helpers.
16. Names, docs, and one `PORTING.md` entry per divergence.
17. WaOS: the gallery and two-windows tests against it.
18. A ticker follows its widget in the mixins' `activate` (decision 16): a fixed view per callback, `Ticker::set_view`, and every State with a ticker mixin forwarding `activate` to it.

## Research behind it

- Flutter's single frame: `engine/src/flutter/shell/platform/darwin/macos/framework/Source/FlutterEngine.mm` (`onVSync`), `flutter_windows_engine.cc` (`OnVsync`), and the framework's `drawFrame` in `rendering/binding.dart` and `widgets/binding.dart`, in Inset's `.reference/flutter`.
- Per-view build scopes and their special cases (a `GlobalKey` move across views, render objects never laid out, nested scopes, global state per frame) were checked by experiment in a copy of Inset; the findings decided option II.
- How far Flutter has settled (2026-10-22), from its code and its issues and design docs: one vsync for all views is the stated consensus of #142845; the multiple-views design (flutter.dev/go/multiple-views) accepts one refresh rate for every view; the multi-view pipeline design (flutter.dev/go/multi-view-pipeline-and-rasterizer) keeps one pipeline item for every view; #146249 (open) proposes a vsync per display; Canonical's windowing design (flutter.dev/go/multi-window-api) keeps the app lifecycle app-wide and per-window state on window controllers; #192128 (commit `6d2efe7eb8c`, reverted by `0b771a58421`) presents only the views that painted, and marks every view for a frame the engine asks for.
- Frames per window elsewhere, read in their spec or source: the HTML standard's "update the rendering" runs only the documents with a rendering opportunity, from their display's refresh, and drops hidden ones; GTK's `gtk_widget_get_frame_clock` returns its surface's frame clock (`gtk/gtkwidget.c`); Android's `ViewRootImpl.scheduleTraversals` posts each window's traversal to its thread's `Choreographer`.
