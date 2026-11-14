//! The home's pill: a search field, where a program's pill names its page, as Safari has its
//! address field. What is typed in it is what the library's home searches its programs for, so
//! the grid of the home's page filters as the user types.
//!
//! Until the field has the keyboard it takes no touch of its own: a tap on the pill gives it the
//! keyboard, and a drag on it is the bar's, sideways along the strip or upward into the overview,
//! as on any pill. With the keyboard, the field edits as a text field does, the bar sits above
//! the keyboard, and the field gives the keyboard up when the user submits, touches anywhere
//! else, or drags the bar. The field's text outlives the pill, which is there only while the
//! home's page is on the strip, and is cleared when the home's tab is gone or a program's.
//!
//! The invariant: the library searches for exactly what the field holds.

use waft::ui::TextInputAction;
use waft::{
    App, BorderRadiusGeometry, BoxDecoration, BuildContext, DecoratedBox, EdgeInsetsGeometry,
    FocusNode, FocusNodeLeaf, GestureDetector, Handle, Icon, IgnorePointer, IntoWidget, Listenable,
    Listener, Padding, State, TextEditingController, TextEditingControllerLeaf, TextStyle,
    UnfocusDisposition, WidgetRef,
};
use waft_cupertino::{CupertinoColors, CupertinoIcons, CupertinoTextField, OverlayVisibilityMode};
use waft_rendering::HitTestBehavior;

use super::DeckViewState;
use super::bar::{PILL_COLOR, PILL_RADIUS};
use crate::ui::library::home::cupertino::resolved;
use crate::ui::library::words as library_words;

/// The side of the magnifier before what is typed.
const ICON_SIDE: f64 = 18.0;

/// The home's search field: what is typed in it, and what gives it the keyboard.
#[derive(Clone, Copy)]
pub(super) struct SearchField {
    text: Handle<TextEditingController>,
    focus: Handle<FocusNode>,
}

impl DeckViewState {
    /// The search field, empty, telling the library what is typed in it from now on, and the
    /// deck when it gains or loses the keyboard.
    pub(super) fn new_search_field(self: Handle<Self>, app: &mut App) -> SearchField {
        let text = TextEditingController::new(app);
        text.as_controller()
            .add_listener(app, Listener::new(move |app| self.search_typed(app)));
        let focus = FocusNode::new(app);
        focus
            .as_node()
            .add_listener(app, Listener::new(move |app| self.redraw(app)));
        SearchField { text, focus }
    }

    /// Lets go of the search field for good.
    pub(super) fn dispose_search_field(self: Handle<Self>, app: &mut App) {
        if let Some(search) = app.get_mut(self).search.take() {
            app.get_mut(search.text).dispose();
            app.destroy(search.text);
            search.focus.dispose(app);
        }
    }

    fn search_field(self: Handle<Self>, app: &App) -> SearchField {
        app.get(self).search.expect("created in init_state")
    }

    /// The library searches for what the field holds.
    fn search_typed(self: Handle<Self>, app: &mut App) {
        if !self.mounted(app) {
            return;
        }
        let typed = self.search_field(app).text.text_value(app).to_owned();
        let library = self.widget(app).library.clone();
        library.update(app, |library, cx| library.search_for(&typed, cx));
    }

    /// Whether the search field has the keyboard.
    pub(super) fn search_has_the_keyboard(self: Handle<Self>, app: &mut App) -> bool {
        self.search_field(app).focus.as_node().has_focus(app)
    }

    /// The search field gives the keyboard up, where it has it.
    pub(super) fn leave_search(self: Handle<Self>, app: &mut App) {
        if self.search_has_the_keyboard(app) {
            let focus = self.search_field(app).focus;
            focus.as_node().unfocus(app, UnfocusDisposition::Scope);
        }
    }

    /// Empties the search field, and so the library's search.
    pub(super) fn clear_search(self: Handle<Self>, app: &mut App) {
        self.search_field(app).text.clear(app);
    }

    /// The home's pill.
    pub(super) fn search_pill(
        self: Handle<Self>,
        app: &mut App,
        context: BuildContext,
    ) -> WidgetRef {
        let search = self.search_field(app);
        let has_the_keyboard = self.search_has_the_keyboard(app);
        let label = resolved(CupertinoColors::LABEL, app, context);
        let secondary = resolved(CupertinoColors::SECONDARY_LABEL, app, context);
        let magnifier = Padding::new(EdgeInsetsGeometry::from_ltrb(12.0, 0.0, 0.0, 0.0)).child(
            Icon::new(Some(CupertinoIcons::search()))
                .size(ICON_SIDE)
                .color(secondary),
        );
        let field = CupertinoTextField::borderless()
            .controller(search.text.as_controller())
            .focus_node(search.focus)
            .placeholder(library_words::SEARCH_PROGRAMS)
            .prefix(magnifier)
            .clear_button_mode(OverlayVisibilityMode::Editing)
            .text_input_action(TextInputAction::Search)
            .max_lines(Some(1))
            .style(TextStyle::new().font_size(16.0).color(label))
            .on_submitted(move |app, _| self.leave_search(app))
            .on_tap_outside(move |app, _| self.leave_search(app));
        let surface = BoxDecoration {
            color: Some(resolved(PILL_COLOR, app, context).into()),
            border_radius: Some(BorderRadiusGeometry::circular(PILL_RADIUS)),
            ..BoxDecoration::default()
        };
        let pill = DecoratedBox::new(surface).child(waft::Center::new().child(field));
        GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .on_tap(Listener::new(move |app| {
                self.search_field(app).focus.request_focus(app, None);
            }))
            .child(IgnorePointer::new().ignoring(!has_the_keyboard).child(pill))
            .into_widget()
    }
}
