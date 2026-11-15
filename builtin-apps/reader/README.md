# reader

A feed reader, as a Waft app in WinUI's Fluent look over `wasdk-adapter-waft`: the app a person keeps open to follow a few sites, and what the project learns WaOS's network, pictures and long text from.

- **Feeds** are followed by address, RSS or Atom (and JSON Feed, which `feed-rs` reads too): Add Feed takes a feed's own address or that of a site whose page names one, fetches it while its dialog waits, and says why where it cannot be added. The first launch follows three public feeds, so the lists show something: NASA's news, the Rust blog and Wikipedia's featured articles. A feed is unfollowed in Settings, after a question.
- **The items** of every feed, or of one, are listed newest first: the title, in strong type with a dot until it is read, the first lines of its text, the feed and how long ago it was published, and its picture. Mark All as Read marks what is listed.
- **An item** opens in the app and is marked read: its HTML read into headings, paragraphs, lists, quotations, code and pictures and laid out in one column, not shown by a browser. A link in its text, and Open in Browser, open in the system's browser; Mark as Unread goes back with the item unread.
- **A refresh** fetches each feed once: at launch, as the app comes back to the front, every half hour while it runs, in sight or not, and when the user asks. It sends what the server said of the document last time, so an unchanged feed costs a `304`; a feed the app itself fetched within ten minutes is left alone. With no network the lists show what the library has, under a bar that says the feed could not be refreshed, with Try Again.
- **A notification** says how many new items a refresh on the app's schedule found, and in which feeds, where the user turned that on in Settings; a click on it brings the window forward.
- **Kept** in the program's `/data`: `feeds.json`, `items/<feed>.json` with the read marks, `settings.json`. Pictures are kept as fetched in `/cache/pictures`, trimmed to 150 MB at launch.

What it does not do: folders of feeds, search, starring, OPML import or export, a menu bar and keyboard shortcuts of its own, a feed's own icon, video or audio enclosures, tables as grids, selecting an item's text. A refresh happens only while the program runs: nothing in WaOS wakes a program on a schedule.

What was not tried: a notification posted and clicked (the permission's question cannot be answered from `cargo waos`), a phone's Back on a phone (the window was checked at a phone's width on a desktop), and the system's dark look.

- **Takes:** `waft` and `waft-winui`, with `waft-gestures` and `waft-services` for a link's tap and cursor; `wasdk-adapter-waft` and `wasdk`, whose `wasdk::http` every request goes through; `feed-rs` for a feed's document, `scraper` for an item's HTML, `url`, `chrono`, `serde` and `serde_json`.
- **Provides:** a component, from `cargo waos build -p reader`, and a package, from `cargo waos package -p reader`.
- **Used by:** people, from WaOS's library.

```bash
cargo waos run -p reader
cargo test -p reader        # the logic, on this computer
```

## How it is made

Three layers, as Waft's README recommends. **Logic** names no `App`: `feed`, `library` (the feeds, the items, the read marks, what changed), `store` and `safe_write` (the files), `feed_address`, `feed_discovery`, `feed_document` (a document into items), `article` and `article_html` (an item's HTML into blocks), `dates`, `picture_cache`, `settings`, `starting_feeds`, `browsing` (what the window shows). **State** is entities over it: `reader_app`, `refreshing`, `fetching`, `pictures`, `adding`, `unfollowing`, `showing`, `new_item_notices`, `opening_links`. **UI** is thin widgets: `main_page`, `feed_list`, `header`, `content`, `notices`, `item_list`, `item_row`, `picture_view`, `reading_page`, `article_view`, `rich_text`, `settings_page`, `question`, `bar_button`, `icons`, `window`.

`http` is the one place that touches the network: `http::get` follows redirects, which WaOS answers rather than follows, and ends a request by a deadline and a length; `http/exchange` is one request over `wasdk::http`, WASI 0.3's client, awaited without stopping the window, and given up at its deadline by dropping it, which cancels it in WaOS.

A picture is fetched as its row or its place in the text is first built, four at a time, the one asked for last first; decoded by the system's image codecs (`image-codecs`) scaled down to the size it is shown at; and held while it is among the sixty list pictures or eight full pictures asked for most recently.

The window is a `NavigationView`: the feeds are its pane, a sidebar from 640 points wide and a pane over the content below that; an item being read and the settings show over the items in its content, which stay built beneath, so going back finds the list where it was left.
