//! Unfollowing a feed: the app asks first, since the feed's items and their read marks go with
//! it, then forgets the feed everywhere it is known.

use waft::App;

use crate::feed::FeedId;
use crate::question::{Question, ask};
use crate::reader_app::ReaderApp;
use crate::refreshing::forget_feed;

/// Asks whether to unfollow the feed `id`, and unfollows it where the user says so.
pub(crate) fn ask_to_unfollow(app: &mut App, reader: &ReaderApp, id: FeedId) {
    let Some(title) = reader
        .library
        .read(app)
        .feed(id)
        .map(|feed| feed.title.clone())
    else {
        return;
    };
    let question = Question {
        title: format!("Unfollow “{title}”?"),
        message: "Reader forgets its items, and which of them you have read.".to_owned(),
        confirm: "Unfollow",
    };
    let answer = ask(app, &reader.navigator, question);
    let reader = reader.clone();
    app.spawn(async move |cx| {
        if answer.await {
            cx.update(|app| unfollow(app, &reader, id));
        }
    });
}

fn unfollow(app: &mut App, reader: &ReaderApp, id: FeedId) {
    reader.change_library(app, |library| library.unfollow(id));
    forget_feed(app, reader, id);
    reader.browsing.update(app, |browsing, cx| {
        browsing.feed_gone(id);
        cx.notify();
    });
}
