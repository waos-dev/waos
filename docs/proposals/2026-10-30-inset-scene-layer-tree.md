# Inset's scene as Flutter's layer tree

**Proposal (2026-10-30), for the lead to walk through.** Decision 2 of `2026-10-30-native-surfaces.md` chose it: a frame's scene keeps its layers, each native surface a layer of its own, and the host walks the tree, places each surface, draws the pictures with valo making room at each surface, and draws nothing when nothing besides surfaces was painted. This proposes the shape: the scene's layer types, what `Scene`, `EngineLayer` and `add_retained` become, what reaches a host for each surface, the host's walk, the points where the parts meet, the size of the change in steps, and the questions left. Nothing in it is decided beyond the two decisions, and nothing in Inset or WaOS was changed.

Every claim is marked **[read]** (read in the source cited) or **[inferred]** (concluded from what was read, not run). Sources: Inset `origin/main` at `1d70c21`, read with `git show` in `/Users/mac/code/inset-rs-2`; Flutter at `ed21324` (2026-08-24) in `/Users/mac/code/flutter`, engine paths relative to `engine/src/flutter`; valo 0.13.1 (`valo-dl` in the cargo registry); WaOS `main` at `5a64bbf`.

## In one paragraph

`SceneBuilder` builds a tree of layers that never change once built: each push opens a container (a transform, a clip, an opacity, a backdrop filter), `pop` closes it and returns it as the `EngineLayer`, `add_picture` adds a picture and `add_surface` adds a surface at its place in paint order; `Scene` is the tree's root. A host calls `scene.composite()`, Inset's port of Flutter's `LayerTree::Preroll` and `Paint`: it walks the tree in paint order with a stack of the containers open, and gives back a `Composition`, the pictures between surfaces (slices) and the surfaces, each surface with its rectangle and the transforms, clips and opacity above it (Flutter's mutators). The host places the surfaces in its compositor, draws the slices with valo with a hole at each surface (`Composition::picture_with_room`), and draws nothing when no slice paints (`Composition::paints`). valo gains nothing.

```text
framework, UI thread                 inset-embedder                          host, in View::present
  layers' add_to_scene ──▶ SceneBuilder ──▶ Scene: a tree ──▶ scene.composite() ──▶ Composition
                                                                 surfaces + mutators ──▶ the compositor places them
                                                                 slices ──▶ valo: one picture, a hole at each surface
                                                                           (nothing, when no slice paints)
```

## Today

- **The builder flattens.** Every push opens a nested valo recording, `pop` closes it into a display list embedded in the enclosing one and returns that list as the `EngineLayer` (`pub type EngineLayer = Arc<Picture>`), `add_retained` embeds the same list again, and `build` gives `Scene { picture, surfaces }` **[read: `crates/inset-embedder/src/scene_builder.rs`]**. The builder tracks the transform and the bounds of the clips pushed, so `add_surface` records a `SurfacePlacement` of the surface's bounding box in the view's pixels, the horizontal scale of the transforms and the clip's bounding box, beside the picture **[read: same, `add_surface`, `in_view_coordinates`, `horizontal_scale_of`]**. Its `PORTING.md` records the cost: "a rotated surface is placed by its bounding box, a rounded clip's corners show the surface unless the picture paints over them, and an opacity above it is ignored" **[read: `crates/inset-embedder/src/PORTING.md`]**.
- **The widget paints the hole.** `RenderSurfaceView::paint` draws its box with `BlendMode::Clear` and adds a `SurfaceLayer`; it always needs compositing, so every transform and clip above it is a layer **[read: `crates/inset-rendering/src/surface_view.rs`]**. The framework's `SurfaceLayer` answers `always_needs_add_to_scene`, because "a retained display list carries no surface", so every ancestor of a surface is added again each frame **[read: `crates/inset-rendering/src/layer.rs`]**.
- **A hole inside a layer shows nothing.** valo lets an enclosing group's alpha ride a draw only when its paint blends `SrcOver` **[read: valo-dl `paint.rs`, `takes_group_opacity`]**, so an opacity below 255, a backdrop filter or a clip with `AntiAliasWithSaveLayer` above a surface gives its clear a texture of its own, which composites over what lies beneath it, and the surface does not show **[inferred; PR #11's `rendered_contents.rs` says the same: "A layer composites what is cleared in it over what lies beneath: no hole shows through"]**.
- **Holes cannot be told from drawing.** PR #11 (`draw-only-what-paints`) answers "only holes" by walking the ops for upright cleared rectangles in no layer **[read: `crates/inset-embedder-winit/src/rendered_contents.rs` on that branch]**.
- **Hosts.** The winit host places `scene.surfaces` through `WindowSurfaces::place` (Core Animation on macOS and iOS: three layers per surface, a clip layer, a content layer scaled by a transform, the buffer's layer) and draws `scene.picture` with valo **[read: `view.rs`, `rasterizer.rs`, `os/apple/composition/`]**; Windows and Linux place nothing (`os/absent.rs`) **[read]**. Android, the web host, `inset-winui-test-support` and WaOS's `inset-embedder-wasdk` draw `scene.picture` alone, and `inset-test` keeps the scene for tests to read **[read]**. A surface's present places its children beneath its buffer as the same `SurfacePlacement`s (`SurfaceFrame::children`), which is how a WaOS program's surfaces are to sit in its picture **[read: `crates/inset-embedder/src/composition.rs`; `2026-10-29-media.md`, decision 7]**.

## What Flutter does

- **Building.** dart:ui's `SceneBuilder` starts with a root `ContainerLayer`; each push makes a container of its kind, adds it to the open one and opens it, and wraps it at once as the Dart `EngineLayer` (`EngineLayer::MakeRetained`); `pop` closes it; `addRetained` adds the same `shared_ptr` to the open container; `addPicture` adds a `DisplayListLayer`, `addPlatformView` a `PlatformViewLayer(offset, size, view_id)`; `build` makes the `Scene` over the root **[read: `lib/ui/compositing/scene_builder.cc`]**. `pushOffset` is a `TransformLayer` with a translation **[read: same]**. `oldLayer` only passes the old layer's id on for the diff (`AssignOldLayer`) **[read: same; `flow/layers/layer.h`]**.
- **The layers.** `ContainerLayer`; `TransformLayer`; `ClipRectLayer`, `ClipRRectLayer`, `ClipRSuperellipseLayer`, `ClipPathLayer`; `OpacityLayer` (alpha and offset); `ColorFilterLayer`; `ImageFilterLayer`; `BackdropFilterLayer` (filter, blend mode, backdrop id); `ShaderMaskLayer`; and the leaves `DisplayListLayer`, `PlatformViewLayer`, `TextureLayer`, `PerformanceOverlayLayer` **[read: `flow/layers/`]**.
- **Preroll.** `LayerTree::Preroll` walks the tree with a `LayerStateStack` of the transforms, clips, opacities and filters open **[read: `flow/layers/layer_tree.cc`]**. `PlatformViewLayer::Preroll` fills a `MutatorsStack` from it and hands `EmbeddedViewParams(matrix, size, mutators)` to the embedder's `PrerollCompositeEmbeddedView` **[read: `flow/layers/platform_view_layer.cc`]**. The mutators are clip rect, rounded rect, superellipse and path, transform, opacity, and a backdrop filter **[read: `flow/embedded_views.h`, `MutatorType`]**; colour and image filters push none ("There is no ImageFilter mutator currently") **[read: `flow/layers/layer_state_stack.cc`]**. `BackdropFilterLayer::Preroll` pushes its filter to the platform views visited before it (`PushFilterToVisitedPlatformViews`) **[read: `flow/layers/backdrop_filter_layer.cc`]**.
- **Paint.** `PlatformViewLayer::Paint` sets `context.canvas = view_embedder->CompositeEmbeddedView(view_id_)` and `state_stack.set_delegate(canvas)` **[read]**, which restores the old canvas to where it started and reapplies every open entry on the new one; a backdrop filter is reapplied as a plain save layer, "not perfect if the BlendMode is not associative" **[read: `layer_state_stack.cc`, `set_delegate`, `decommission`, `BackdropFilterEntry::reapply`]**. Each slice is a display list builder **[read: `flow/embedded_views.h`, `DisplayListEmbedderViewSlice`]**.
- **Embedders.** The embedder API passes opacity, clip rect, clip rounded rect and transform, approximates a superellipse, and drops a clip path ("Unsupported mutation") and backdrop filters **[read: `shell/platform/embedder/embedder_layers.cc`]**. macOS applies the transforms as one `CATransform3D`, `layer.opacity` as the product of opacities, the clip rects as one master clip and the rounded rects as path masks where a corner cuts the view **[read: `shell/platform/darwin/macos/framework/Source/FlutterMutatorView.mm`]**. iOS also masks by a path, multiplies the view's `alpha`, and applies a backdrop filter only when it is a blur, through a `UIVisualEffectView` **[read: `shell/platform/darwin/ios/framework/Source/FlutterPlatformViewsController.mm`]**.
- **Retained layers.** The retained node is the same object in the next tree, walked again by Preroll and Paint every frame **[read: `scene_builder.cc`, `layer_tree.cc`]**. What retaining saves: the framework skips `addToScene` for the subtree, the diff skips a node that is the same instance and keeps its paint region **[read: `flow/layers/container_layer.cc`, `DiffChildren`]**, and the raster cache keys a layer by its id **[read: `flow/layers/layer_raster_cache_item.cc`]**.
- **The framework.** `PlatformViewLayer.addToScene` calls `addPlatformView` and nothing else, and does not override `alwaysNeedsAddToScene`, so a subtree holding a platform view is retained like any other **[read: `packages/flutter/lib/src/rendering/layer.dart`]**.

## The shape

### The scene's layers

In `inset-embedder`, a `scene` module in place of `scene_builder.rs`. A node is immutable and shared, so a retained subtree is the same node in the next frame's tree **[inferred]**.

```rust
/// One node of a frame's layer tree: the engine's `flow::Layer`. Equal as a host asks it: a
/// container and a surface by pointer first, as today's placements compare their surfaces.
#[derive(Clone)]
pub enum Layer {
    /// Children under one effect: what a `push_*` opens and `pop` closes.
    Container(EngineLayer),
    /// A recorded picture at an offset: flow's `DisplayListLayer`, from `add_picture`.
    DisplayList(DisplayListLayer),
    /// A native surface at its place in paint order: flow's `PlatformViewLayer`, from
    /// `add_surface`. It draws nothing; the host places the surface and makes room for it.
    Surface(SurfaceLayer),
}

/// dart:ui's `EngineLayer`: a container of the tree, which `pop` returns and `add_retained`
/// adds again. Compared by identity first, so a retained subtree compares at once.
pub type EngineLayer = Arc<ContainerLayer>;

/// flow's `ContainerLayer` and the containers derived from it: children in paint order, under
/// one effect.
pub struct ContainerLayer {
    pub effect: Effect,
    pub children: Vec<Layer>,
}

/// Which of flow's containers a container is: one role each.
pub enum Effect {
    /// `ContainerLayer` itself: the scene's root.
    None,
    /// `TransformLayer`: `push_transform`, and `push_offset` as a translation.
    Transform(Matrix4),
    /// `ClipRectLayer`, `ClipRRectLayer`, `ClipPathLayer`.
    ClipRect(Rect, Clip),
    ClipRRect(RRect, Clip),
    ClipPath(Arc<Path>, Clip),
    /// `OpacityLayer`: the children at `alpha`, moved by `offset`.
    Opacity { alpha: u8, offset: Offset },
    /// `BackdropFilterLayer`: what lies beneath filtered before the children paint.
    BackdropFilter { filter: ImageFilter, blend_mode: BlendMode, backdrop_id: Option<u64> },
}

pub struct DisplayListLayer {
    pub offset: Offset,
    pub picture: Arc<Picture>,
}

pub struct SurfaceLayer {
    pub surface: Arc<dyn Surface>,
    /// Where the surface goes, in its container's coordinates.
    pub rect: Rect,
}
```

These are the layers Inset's framework adds today **[read: `inset-rendering/src/layer.rs`]**. Flutter's colour filter, image filter, shader mask, superellipse clip and texture layers are deferred in `inset-rendering`'s `PORTING.md` **[read]**; each joins as an `Effect` or a leaf when its framework layer is ported, with no change to the shape **[inferred]**. The two hints of `add_picture` stay accepted and unused, as today **[read: `add_picture`]**.

### `Scene`, `EngineLayer` and `add_retained`

- `Scene` is the tree: `pub struct Scene { pub root: EngineLayer }`, the root's effect `None`. Flutter's `LayerTree` also holds the frame size **[read: `layer_tree.cc`]**; Inset's view knows its own **[inferred]**. It stays `PartialEq`, so a host still tells a scene equal to the one it shows (`LastScene::shows`): a retained container compares by pointer, a picture as valo's lists do, "a list is equal to itself without a comparison" **[read: valo-dl `list.rs`]**.
- `EngineLayer` is a container node, as dart:ui's wraps flow's `shared_ptr<ContainerLayer>` **[read]**, known at `pop` as today. `add_retained(&layer)` adds the same node to the open container. The framework's retained-rendering bookkeeping (`needs_add_to_scene`, `engine_layer`, `add_to_scene_with_retained_rendering`) is unchanged **[read: `layer.rs`]**; only what it holds changes.
- The framework's `SurfaceLayer` loses `always_needs_add_to_scene`: the walk computes a surface's placement from the tree each frame, so a retained subtree places its surfaces again, as Flutter's does **[inferred]**.
- No `oldLayer`: Flutter uses it for its diff only **[read]**, and Inset has no diff **[inferred]**.

### What reaches a host for a surface

```rust
/// Where one frame puts one native surface, with everything above it: Flutter's
/// `EmbeddedViewParams`. Also what a surface frame's children are (`SurfaceFrame::children`).
/// Equal when it is the same surface, by pointer, with the same rectangle and mutators.
#[derive(Clone)]
pub struct SurfacePlacement {
    pub surface: Arc<dyn Surface>,
    /// The surface's rectangle, in the coordinates the last transform leaves.
    pub rect: Rect,
    /// What the containers above it do, the root's first: Flutter's `MutatorsStack`. The first is
    /// the root's transform, which takes logical pixels to the view's physical ones.
    pub mutators: Vec<Mutator>,
}

/// One thing a container above a surface does to it: Flutter's `Mutator`.
#[derive(Clone, Debug, PartialEq)]
pub enum Mutator {
    Transform(Matrix4),
    ClipRect(Rect),
    ClipRRect(RRect),
    ClipPath(Arc<Path>),
    Opacity(f32),
}

impl SurfacePlacement {
    /// Every transform in one: Flutter's `transformMatrix`.
    pub fn transform(&self) -> Matrix4;
    /// Every opacity multiplied.
    pub fn opacity(&self) -> f32;
    /// How many of the view's pixels one unit of the surface spans along its own horizontal: what a
    /// producer's buffer is shown at, today's `scale`.
    pub fn scale(&self) -> f64;
    /// The box around the surface under `transform`, in the view's pixels: today's `rect`.
    pub fn bounds(&self) -> Rect;
    /// `bounds` cut to the bounds of every clip: today's `clip`, for a host that clips to
    /// rectangles only.
    pub fn visible_bounds(&self) -> Rect;
}
```

The offset of an `OpacityLayer` is a transform before its opacity, as Flutter's `OpacityLayer::Preroll` translates and then applies the opacity **[read: `flow/layers/opacity_layer.cc`]**. No backdrop-filter mutator: open question 3.

## The host's walk

### Composing

`Scene::composite(&self) -> Composition` is the one walk every host uses (open question 1). It goes through the tree in paint order with a stack of the containers open, recording into a valo builder as today's `SceneBuilder` records: a transform as `save` and `concat`, a clip as `save` (or `save_layer` for `AntiAliasWithSaveLayer`) and the clip, an opacity as `save_layer` with the alpha and a translation, a backdrop filter as `save_layer_backdrop`, a picture as `draw_display_list` at its offset **[read: today's push and add bodies]**. At a surface it does what Flutter's canvas switch does: it closes every open scope, so what was painted becomes a slice of its own; it records the surface with the mutators the stack holds; and it reopens every scope for the next slice, a backdrop filter as a plain save layer of its blend mode, as Flutter's `reapply` does **[read for Flutter; inferred for Inset]**.

```rust
/// A scene in paint order, for a host that shows surfaces beside its own drawing: what Flutter's
/// embedder hands its compositor as `FlutterLayer`s, slices not yet rendered.
pub struct Composition {
    pub parts: Vec<Part>,
}

pub enum Part {
    /// What the scene paints between two surfaces, every scope open at its ends closed in it.
    Slice(Arc<Picture>),
    Surface(SurfacePlacement),
}

impl Composition {
    pub fn surfaces(&self) -> impl Iterator<Item = &SurfacePlacement>;
    /// Whether anything besides surfaces was painted.
    pub fn paints(&self) -> bool;
    /// The slices as one picture, with room made at each surface: what a host draws that shows
    /// every surface beneath its one buffer.
    pub fn picture_with_room(&self) -> Picture;
}
```

The walk visits every layer every frame, as Flutter's Preroll and Paint do, where today a retained subtree is one embedded list that is not visited **[read]**. If that shows in a frame's time, a retained container with no surface beneath it can be recorded once and embedded by its node; that is measured first, in an experiment, rather than built now **[inferred]**.

### Placing surfaces

The host places each surface from its placement, bottom-most first, in the transaction it presents the frame in, as today **[read: `WindowSurfaces::place`]**. With the mutators the Core Animation host can do what Flutter's macOS embedder does: the whole transform as a `CATransform3D` on the content layer instead of its horizontal scale, rounded and path clips as a mask on the clip layer (a `CAShapeLayer`) instead of their bounds, and the opacity (open question 2) **[read for Flutter; inferred for Inset]**. Until then it reads `bounds()`, `scale()` and `visible_bounds()`, today's rectangle, scale and clip, and nothing it shows changes (step 1).

### Drawing with room made

`picture_with_room` draws each slice in order and, at each surface, a hole: the mutators' transforms and clips replayed into the builder, and the surface's rectangle cleared under them (by the opacity above it, if open question 2 goes to (b)). The hole is drawn between slices, so it is in no layer and reaches the buffer, which fixes the surface under an opacity or a backdrop filter that today shows nothing **[inferred]**; it is cut by every clip exactly, rounded and path clips included, since valo clips by the shape **[read: valo-dl `Op::ClipPath`]**; and it turns with any transform. The surface stays beneath the one buffer, as decision 3 of the media proposal and decision 1 of the native-surfaces one keep it **[read]**. A host that later wants Flutter's overlay buffers above surfaces renders each slice into a buffer of its own instead, from the same `Composition` **[inferred]**.

### Nothing painted besides surfaces

`paints` answers whether any slice draws: valo counts a list's draws as it records them, nested lists included (`DisplayList::draw_count`), and its backdrop reads (`backdrop_reads`) **[read: valo-dl `list.rs`]**. A slice with neither changes no pixel of the buffer **[inferred]**. With no hole painted by a widget, nothing has to be recognised by its shape, so PR #11's `rendered_contents.rs` goes **[inferred]**. Flutter asks the same per slice through `DlOpSpy`, which also passes over draws with a transparent paint **[read: research §1, `shell/common/dl_op_spy.cc`]**; valo's count does not, so such a draw is drawn **[inferred]**. When a frame paints nothing, the host places the surfaces and draws nothing; if its buffer shows a frame that did paint, it draws one empty frame, so the old drawing does not stay over the surfaces **[inferred]**.

### Opacity, filters and backdrops over a surface

| Above a surface | Flutter | Where Flutter falls short | Here |
|---|---|---|---|
| Transform | a mutator; macOS and iOS apply the whole matrix **[read]** | — | a mutator; the hole is drawn under it; Core Animation applies it whole (step 4) |
| Clip rect, rounded rect | mutators; macOS masks a rounded rect only where a corner cuts the view **[read]** | — | mutators; the hole is exact; Core Animation masks the rounded ones (step 4) |
| Clip path | iOS masks by it; the embedder API drops it, so macOS shows the view unclipped **[read]** | macOS, Windows, Linux | a mutator; the hole is exact; Core Animation masks by the path (step 4) |
| Opacity | a mutator: the view's layer at the product of the opacities; what the same group paints after the view goes into the next slice under the group's save layer again **[read]** | a group split at a view composites in two parts, which differs where the parts overlap **[inferred; Flutter says as much of a backdrop layer reapplied: "not perfect if the BlendMode is not associative"]** | the same split; how the opacity is shown in an underlay is open question 2 |
| Colour filter, image filter, shader mask | no mutator **[read]** | the view shows unfiltered | the same; Inset has none of these layers yet **[read]** |
| Backdrop filter | pushed to the views beneath it; iOS applies a blur only, through `UIVisualEffectView`; the embedder API drops it **[read]** | everything but a blur on iOS | the filter reads the buffer, where the surface is a hole, so the glass filters only the framework's pixels and the surface shows through it unfiltered **[inferred]**; open question 3 |

## Where the parts meet

**1. A widget adds its layer** (`inset-rendering/src/surface_view.rs`, the framework's paint phase, UI thread). The box paints nothing; its layer splits the picture there, since `add_layer` stops the recording before it **[read: `painting_context.rs`, `add_layer`]**.

```rust
fn paint(self: RenderHandle<Self>, app: &mut App, context: &mut PaintingContext, offset: Offset) {
    let surface = Arc::clone(&self.get(app).surface);
    let layer = SurfaceLayer::new(app, surface, offset & self.size(app));
    context.add_layer(app, layer.as_layer());
}
```

The framework's layers call the builder as they do today, for example `OpacityLayer::add_to_scene` (`inset-rendering/src/layer.rs`, from `RenderView::composite_frame`):

```rust
builder.push_opacity(realized_alpha, offset);
self.as_container_layer().add_children_to_scene(app, builder);
let engine_layer = builder.pop();
self.as_layer().set_engine_layer(app, Some(engine_layer));
```

**2. `SceneBuilder` builds the tree** (`inset-embedder/src/scene/builder.rs`, same call, same thread). It no longer tracks transforms or clips; the walk does.

```rust
pub fn push_opacity(&mut self, alpha: i32, offset: Offset) {
    self.open.push(OpenContainer::new(Effect::Opacity { alpha: alpha.clamp(0, 255) as u8, offset }));
}

pub fn pop(&mut self) -> EngineLayer {
    assert!(self.open.len() > 1, "SceneBuilder::pop without a push");
    let closed = self.open.pop().expect("the root stays open");
    let layer = Arc::new(ContainerLayer { effect: closed.effect, children: closed.children });
    self.add(Layer::Container(Arc::clone(&layer)));
    layer
}

pub fn add_retained(&mut self, retained_layer: &EngineLayer) {
    self.add(Layer::Container(Arc::clone(retained_layer)));
}

pub fn add_surface(&mut self, surface: Arc<dyn Surface>, rect: Rect) {
    self.add(Layer::Surface(SurfaceLayer { surface, rect }));
}
```

**3. The walk** (`inset-embedder/src/scene/composition.rs`, called by the host inside `View::present`).

```rust
fn visit(&mut self, layer: &Layer) {
    match layer {
        Layer::Container(container) => {
            self.open.push(&container.effect, &mut self.slice); // save, concat, clip or save_layer
            for child in &container.children {
                self.visit(child);
            }
            self.open.pop(&mut self.slice); // restore
        }
        Layer::DisplayList(picture) => self.draw_picture(picture),
        Layer::Surface(surface) => {
            self.end_slice(); // closes every open scope: what was painted is a slice of its own
            self.parts.push(Part::Surface(SurfacePlacement {
                surface: Arc::clone(&surface.surface),
                rect: surface.rect,
                mutators: self.open.mutators(), // transforms, clips and opacities, the root's first
            }));
            self.open.reapply(&mut self.slice); // the next slice opens under the same scopes
        }
    }
}
```

**4. The macOS host** (`inset-embedder-winit`; `WinitView::present` on the main thread, the placing inside the window's `CATransaction`).

```rust
fn present(&self, scene: Arc<Scene>) {
    let mut rasterizer = self.rasterizer.borrow_mut();
    if rasterizer.shows(&scene) {
        return;
    }
    let composition = scene.composite();
    if let Some(native_surfaces) = &*self.native_surfaces.borrow() {
        native_surfaces.place(composition.surfaces(), self.id); // Core Animation
    }
    rasterizer.draw(&scene, &composition); // valo, or nothing when no slice paints
}

// rasterizer.rs
pub(crate) fn draw(&mut self, scene: &Arc<Scene>, composition: &Composition) {
    self.last_scene.hand_over(scene);
    if !composition.paints() && !self.shows_a_painted_frame() {
        self.last_scene.presented(Ok(())); // the surfaces moved; the buffer has nothing new
        return;
    }
    self.draw_picture(&composition.picture_with_room());
}

// os/apple/composition/mod.rs, CoreAnimationSurface::place, from step 4
clip_layer.setFrame(geometry.clip_frame); // the bounds of every clip, as today
clip_layer.setMask(geometry.clip_mask.as_deref()); // a CAShapeLayer for rounded and path clips
content_layer.setTransform(geometry.transform); // the whole matrix, not only its scale
```

**5. WaOS's embedder hands a program's scene on** (`sdk/inset-embedder-wasdk/src/views.rs`, the program's thread inside its component). Today it draws the picture alone **[read]**; the surface half waits for the media proposal's `open-surface` and `place` in the WIT, which take effect with the view's next present **[read: `2026-10-29-media.md`, decision 7]**.

```rust
fn present(&self, scene: Arc<inset_embedder::Scene>) {
    let composition = scene.composite();
    for placement in composition.surfaces() {
        self.surfaces.place(placement); // the WIT's `place`: the rectangle and the mutators
    }
    if composition.paints() || self.shows_a_painted_frame() {
        self.renderer.draw(&composition.picture_with_room(), &self.canvas, self.clear);
    }
    self.view.present(); // the texture, if one was drawn, and the placements, together
}
```

and on the engine's side, where a program's picture is a surface in its window (`crates/waos-program-window/src/logic/canvas_surface.rs`, the engine's frame thread), the program's placements become that surface's children, beneath its buffer, in one transaction **[read: `SurfaceFrame`; inferred for WaOS]**:

```rust
self.surface.present(SurfaceFrame { buffer: picture, children: placements }, released);
```

WaOS already presents nothing when the program drew no texture (`ViewCanvas::present` answers `false`) **[read: `crates/waos/src/pictures/canvas.rs`]**; what a present with placements and no texture shows is the WIT's to say when `place` lands **[inferred]**.

## The size

### What changes

| Where | What changes |
|---|---|
| `inset-embedder`: `scene_builder.rs` (765 lines, from line 419 tests) becomes `scene/` (`layer.rs`, `builder.rs`, `composition.rs`) | the tree; the builder building it; the walk, `Composition`, `SurfacePlacement` with mutators. Its tests move: placement under transforms and clips become mutator tests, the drawn backdrop tests draw `picture_with_room` |
| `inset-embedder`: `composition.rs`, `views.rs`, `lib.rs`, `painting.rs`, `README.md`, `PORTING.md` | `SurfaceFrame::children` holds the new placement; `View::present`'s doc; exports; `flattened_ops` callers; a host's duties in the README ("place a scene's surfaces beneath its picture" becomes the walk's); the `scene_builder.rs` entries of `PORTING.md` |
| `inset-rendering`: `surface_view.rs`, `layer.rs`, `PORTING.md` | the view paints nothing; `SurfaceLayer` is retained like any layer; `layer.rs`'s module doc, which still says no `EngineLayer` is ported though it is **[read]**, is corrected; the tests reading `scene.picture` or `scene.surfaces` (in `layer.rs`, `surface_view.rs`, `painting_context.rs` and `proxy_box.rs`) read the composition |
| `inset-widgets` tests | `container.rs`, `gesture_detector.rs`, `single_child_scroll_view.rs`, `media/player.rs`, `media/camera.rs` read the composition |
| `inset-embedder-winit` | `view.rs` and `rasterizer.rs` as in snippet 4; `os/mod.rs` and `os/absent.rs` take placements from the composition; `os/apple/composition/` (`mod.rs`, `placed.rs`, `window_surfaces.rs`) reads mutators; `PORTING.md` |
| `inset-embedder-android/src/view.rs`, `inset-embedder-web/src/platform.rs` | draw `picture_with_room` (no compositor, so no surface: the slices alone) |
| `inset-test/src/view.rs`, `inset-winui-test-support/src/view.rs`; in the `inset-winui` repository, `examples/winui_gallery/tests/window_resize.rs` | keep the scene; draw `picture_with_room` |
| WaOS: `sdk/inset-embedder-wasdk/src/views.rs` and its README | snippet 5's first half now, the second with media's surfaces |
| WaOS: `crates/waos-program-window/src/logic/canvas_surface.rs` | the children, with media's surfaces |
| PR #11 (`draw-only-what-paints`) | `rendered_contents.rs` replaced by `Composition::paints`; its lazily made device stays (open question 5) |
| PR #12 (`detached-surface-root`) | none: where a root's layer lives is apart from what a scene holds **[inferred]** |
| valo | none |

### Steps, each building and passing on its own

1. **The scene keeps its layers.** `inset-embedder`'s `scene` module; the walk makes one slice and no hole, so `picture_with_room` is today's picture, the widget still paints its clear, and the Core Animation host reads `bounds()`, `scale()` and `visible_bounds()` as today's rectangle, scale and clip. Every consumer moves from `scene.picture` and `scene.surfaces` to the composition, and nothing drawn changes. New tests: a popped container is a node of its parent, a retained one is the same `Arc` in the next scene, a surface's mutators under transforms, clips and an opacity.
2. **The walk makes room.** Slices close and reopen the open scopes at each surface, `picture_with_room` draws a hole between them, `RenderSurfaceView` paints nothing, and the framework's `SurfaceLayer` is retained like any layer. New drawn tests, on `test_support`'s GPU device: a surface under an opacity shows through, a hole under a rounded clip keeps the corners, a retained subtree's surface is still placed.
3. **Nothing drawn when only surfaces paint.** `Composition::paints`; the winit rasterizer draws nothing, or one empty frame after a painted one. Replaces PR #11's guessing.
4. **Core Animation applies what is above a surface.** The whole transform, rounded and path clips as masks, the opacity as open question 2 settles; `LayerGeometry` tests as today's.
5. **WaOS draws a program's composition.** `inset-embedder-wasdk` draws `picture_with_room` and skips a frame that paints nothing; the placements cross the WIT when the media step adds `place`.

Each step updates the READMEs and `PORTING.md` entries it makes wrong, in Inset and WaOS.

## Open questions

1. **Where the walk lives.** (a) In `inset-embedder`, one walk for every host, which gives back a `Composition` of slices and placements, the shape of Flutter's embedder API (`FlutterLayer`s); (b) a trait each host implements, which the walk calls at each surface for the canvas to go on in, as Flutter's `CompositeEmbeddedView` returns one **[read]**; (c) each host walks the tree itself. **Recommended: (a).** Closing and reopening the open scopes at a surface is the same for every host; a composition is data a test asserts on and a program hands to WaOS; a host that wants overlay buffers later renders each slice into its own. (b) hands a host's canvas into the walk across a borrow for each surface, which fits C++'s pointers better than Rust's borrows **[inferred]**; (c) is five copies.
2. **How an opacity above a surface shows, with the surface beneath the buffer.** (a) Flutter's: the surface's layer at the opacity, the hole cleared whole; what the frame painted beneath the surface is gone under the hole, so the surface fades over whatever lies behind the buffer, the window's background **[inferred]**. (b) The hole carries it: the rectangle cleared by the opacity (`DstOut` at that alpha, as Chromium's translucent hole and Android's `punchHole` alpha **[read: research §1]**), the surface's layer whole; beneath an opaque surface this is exactly the surface at that opacity over what was painted, wherever that is opaque, which on an opaque window it always is, since its frame starts white **[inferred]**. (c) Ignore it, as today. **Recommended: (b)**, with the layer hidden at an opacity of 0. It is wrong only where nothing opaque lies beneath, on a window that shows through, where (a) is the right one.
3. **Backdrop filters, and colour and image filters, over a surface.** (a) None reaches a surface: the glass filters only the framework's pixels, and the surface shows through unfiltered; recorded as a known gap, as Flutter's macOS embedder has it **[read]**. (b) A backdrop-filter mutator pushed to the surfaces beneath, as Flutter's `PushFilterToVisitedPlatformViews`, for hosts to apply what their compositor can. **Recommended: (a).** No host here could apply it yet, and adding the mutator later changes no shape **[inferred]**.
4. **What crosses WaOS's WIT for a program's surface**, when media's `place` is designed. (a) What Flutter's embedder API carries: transform, clip rect, clip rounded rect, opacity, a path clip falling back to its bounds **[read: `embedder_layers.cc`]**; (b) every mutator, paths included, which needs a path in the WIT. **Recommended: (a).** The program's hole is exact either way, so a path's bounds show the surface beyond the path only where the program painted nothing opaque beneath it **[inferred]**.
5. **PR #11.** (a) It lands as it is, and step 3 then replaces its `rendered_contents.rs`; (b) it lands without `rendered_contents.rs`, keeping its lazily made device, and step 3 brings the answer. **Recommended: (b)**, so the hole guessing never lands.
6. **Names.** The scene's surface layer is `SurfaceLayer`, as the framework's already is (Flutter calls both `PlatformViewLayer` **[read]**), and its picture layer flow's `DisplayListLayer`, not the framework's `PictureLayer`; the walk's output is `Composition` with `Part::Slice` and `Part::Surface`. **Recommended:** these, so each crate's name for a thing is Flutter's for the same layer, with Inset's surface for its platform view. The other way is flow's names throughout (`PlatformViewLayer`), which leaves Inset with two words for one thing.

## Sources

### Inset (`origin/main`, `1d70c21`)
- `crates/inset-embedder/src/scene_builder.rs`, `composition.rs`, `views.rs`, `painting.rs`, `README.md`, `PORTING.md`
- `crates/inset-rendering/src/layer.rs`, `surface_view.rs`, `painting_context.rs`, `view.rs`, `PORTING.md`
- `crates/inset-embedder-winit/src/view.rs`, `rasterizer.rs`, `os/mod.rs`, `os/absent.rs`, `os/apple/composition/{mod,placed,window_surfaces}.rs`, `PORTING.md`
- `crates/inset-embedder-android/src/view.rs`, `crates/inset-embedder-web/src/platform.rs`, `crates/inset-test/src/view.rs`, `crates/inset-winui-test-support/src/view.rs`; `inset-winui`'s `examples/winui_gallery/tests/window_resize.rs`
- PR #11 `draw-only-what-paints`: `crates/inset-embedder-winit/src/rendered_contents.rs`; PR #12 `detached-surface-root`

### Flutter (`ed21324`)
- `lib/ui/compositing.dart`, `lib/ui/compositing/scene_builder.cc`
- `flow/layers/layer.h`, `layer_tree.cc`, `container_layer.cc`, `platform_view_layer.cc`, `opacity_layer.cc`, `transform_layer.cc`, `clip_shape_layer.h`, `backdrop_filter_layer.cc`, `layer_state_stack.cc`, `layer_raster_cache_item.cc`; `flow/embedded_views.h`
- `shell/platform/embedder/embedder_layers.cc`, `embedder_external_view_embedder.cc`
- `shell/platform/darwin/macos/framework/Source/FlutterMutatorView.mm`, `shell/platform/darwin/ios/framework/Source/FlutterPlatformViewsController.mm`
- `packages/flutter/lib/src/rendering/layer.dart`

### valo 0.13.1
- `valo-dl`: `src/list.rs` (`Op`, `DisplayList::draw_count`, `backdrop_reads`, equality), `src/paint.rs` (`takes_group_opacity`), `src/builder.rs`

### WaOS (`5a64bbf`)
- `sdk/inset-embedder-wasdk/src/views.rs`, `README.md`; `crates/waos-program-window/src/logic/canvas_surface.rs`; `crates/waos/src/pictures/canvas.rs`; `wit/views.wit`
- `docs/proposals/2026-10-30-native-surfaces.md`, `2026-10-30-native-surfaces-research.md`, `2026-10-29-media.md`
