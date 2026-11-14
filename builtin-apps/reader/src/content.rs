//! What the window shows beside the list of feeds: what the window has to say (`notices`), then
//! the items listed, and over them the item being read or the settings. The items stay built
//! beneath an item being read, so that going back finds the list where it was left.
//!
//! An item being read and the settings are no routes of the navigator, since the list of feeds
//! stays beside them; so that a phone's Back still goes back to the items, the content keeps its
//! route from popping while one of them shows, and goes back itself when a pop is asked for.

use std::rc::Rc;

use waft::{
    App, BuildContext, Column, CrossAxisAlignment, Expanded, IndexedStack, IntoWidget, PopScope,
    StatelessWidget, WidgetRef,
};

use crate::browsing::Page;
use crate::item_list::ItemList;
use crate::notices::Notices;
use crate::reader_app::ReaderApp;
use crate::reading_page::ReadingPage;
use crate::settings_page::SettingsPage;
use crate::showing::back_to_items;

#[derive(Debug)]
pub(crate) struct Content {
    pub(crate) reader: ReaderApp,
}

impl StatelessWidget for Content {
    fn build(&self, app: &mut App, _context: BuildContext) -> WidgetRef {
        let reader = &self.reader;
        let items = ItemList {
            reader: reader.clone(),
        };
        let over_the_items = match reader.browsing.read(app).page() {
            Page::Items => None,
            Page::Reading(item) => Some(
                ReadingPage {
                    reader: reader.clone(),
                    item: item.clone(),
                }
                .into_widget(),
            ),
            Page::Settings => Some(
                SettingsPage {
                    reader: reader.clone(),
                }
                .into_widget(),
            ),
        };
        let on_the_items = over_the_items.is_none();
        let mut pages = vec![items.into_widget()];
        pages.extend(over_the_items);
        let shown = IndexedStack::new()
            .index(Some(i32::from(!on_the_items)))
            .children(pages);
        let column = Column::new()
            .cross_axis_alignment(CrossAxisAlignment::Stretch)
            .children([
                Notices {
                    reader: reader.clone(),
                }
                .into_widget(),
                Expanded::new(shown).into_widget(),
            ]);
        let going_back = reader.clone();
        PopScope::new(column)
            .can_pop(on_the_items)
            .on_pop_invoked_with_result(Rc::new(move |app, popped, _result| {
                if !popped {
                    back_to_items(app, &going_back);
                }
            }))
            .into_widget()
    }
}
