# Who draws subtitles: research

Research for the `media-player`'s text tracks. 2026-11-02. Reading only; nothing was built or run. Marks: **[read]** a doc, header or source was read; **[inferred]** my conclusion from what was read; **[not found]** looked for and not found.

## The answer in short

Apple, Android and Windows each give an app every subtitle the system itself would draw, as data with styling and position. Apple sends styled text, Media3 sends `Cue`s (bitmaps included), and Windows sends text and image cues. The web is the gap. Chrome exposes no in-band text tracks from MP4 or HLS. Safari exposes them but keeps the styling of non-WebVTT cues inside the browser. No page can read the OS caption style. With A, the player has already parsed the cues, so what a program needs from Rust is layout and drawing. Rust has two young pure-Rust ASS renderers and pure-Rust bitmap decoders. It has nothing for WebVTT's cue-positioning rules and no CEA-608/708 screen decoder outside GStreamer. Most precedents draw subtitles themselves, and the larger ones apply the OS caption settings. A must add four things the system path gives for free: reading each OS's caption settings, telling accessibility what is on screen, subtitles in system picture-in-picture and AirPlay, and, on Windows only, subtitles in frames mode.

## 1. Cues from the platform's player

| Platform | How the app gets cues | What arrives | Gaps |
|---|---|---|---|
| Apple | `AVPlayerItemLegibleOutput`; `suppressesPlayerRendering = YES` stops AVPlayer drawing that media [read] | Attributed strings with colours, font, size, bold/italic/underline, alignment, position, line, width, vertical layout, edge style [read]. Default styling already includes the user's caption settings [read]. WebVTT and tx3g can also come as raw samples; CEA-608 only as strings [read] | No bitmap subtitle formats exist on Apple: CoreMedia knows only tx3g, wvtt, c608, c708 and atcc [read]. No ruby or region markup [inferred: absent from `CMTextMarkup.h`]. Whether AVPlayer draws CEA-708 [not found] |
| Android Media3 | `Player.Listener.onCues(CueGroup)` [read] | `Cue`: styled text spans or a bitmap; line, position and size with anchors; window colour, text size, vertical type, shear, z-index [read]. Formats: SSA/ASS, WebVTT, SubRip, tx3g, TTML, PGS, VobSub, DVB, CEA-608, CEA-708 [read] | None against the system: Media3 never draws into the video surface, and `PlayerView` draws the same `Cue` list in a `SubtitleView` [read] |
| Windows | `TimedMetadataTrack` in `ApplicationPresented` mode: the platform does not draw, and `CueEntered` and `CueExited` fire [read] | `TimedTextCue` with region, style (font, colours, outline, ruby) and lines with sub-formats; `ImageCue` with a bitmap, position and extent [read]. Formats: TTML, WebVTT, SubRip, SSA, VobSub, PGS [read] | In-band CEA-608/708 as cues [not found]. Subtitles in HLS or DASH [not found] |
| Web | `TextTrack` in `hidden` mode: cues stay active and events fire, but nothing is drawn [read] | `VTTCue`: text, `getCueAsHTML()`, line, position, size, align, vertical, region [read] | Chrome: no in-band text tracks, and its native HLS has no subtitles yet [read, Chromium of 2026-08-15]. Safari: in-band cues that are not WebVTT become `TextTrackCueGeneric`, which keeps fonts and colours in C++ [read], out of script's reach [inferred]. No page can read the OS caption style [read] |

Windows also has a middle path. In frame-server mode, `MediaPlayer.RenderSubtitlesToSurface` draws the current subtitles into a Direct3D surface the app supplies, and `SubtitleFrameChanged` fires when they change [read].

## 2. Rust crates

Versions are from `cargo search` and `cargo info`. Monthly downloads and dates are from lib.rs and docs.rs; "?" means the page did not show it. "Pure Rust" means no C dependency is listed.

| Crate | What it does | Latest | Per month | `wasm32-wasip2` |
|---|---|---|---|---|
| `subparse` 0.7.0 | SRT, ASS/SSA, MicroDVD, VobSub | 2021-04 | ~1.1k | Pure Rust [inferred] |
| `aspasia` 0.2.1 | SRT, ASS/SSA, WebVTT, MicroDVD | 2024-06 | ~4.8k | Pure Rust [inferred] |
| `subtitler` 2.13.0 | 17 formats, including TTML and SCC; keeps ASS styles | 2026-10 | ~250 | `wasm` feature; the defaults pull in reqwest [read] |
| `webvtt-parser` 1.4.1 | WebVTT, with cue settings | ? | ? | Pure Rust [inferred] |
| `servo-webvtt` 0.7.0 | Servo's WebVTT parser | 2026-10 | ~4k | Pure Rust [inferred] |
| `ttml-subtitle` 0.4.0 | TTML2/IMSC 1.1, parse only: no style inheritance or region geometry [read] | 2026-10 | ? | Pure Rust [inferred] |
| `cea608-types` 0.1.5 | CEA-608 parse/write | 2026-10 | ~4k | Pure Rust [read] |
| `cea708-types` 0.4.1 | CEA-708 parse/write; no window state [read] | 2025-06 | ~11k | Pure Rust [read] |
| `caption-convert` 0.4.0 | CEA-608/708 to WebVTT | 2026 | ? | Pure Rust [inferred] |
| `libbitsub-core` 1.13.2 | Parses and renders PGS, VobSub, DVB | 2026-11 | ~37 | Pure Rust [read] |
| `libass` 0.2.0 | Bindings to C libass | 2020-11 | ? | No: C [read] |
| `rassa` 0.7.1 | Rust rewrite of libass; experimental, 1 star | 2026-08 | ? | On wasm32 it uses harfrust and fontdue instead of FreeType; builds `wasm32-unknown-unknown` [read]; wasip2 [inferred] |
| `ass-renderer` 0.1.2 | ASS on tiny-skia, optionally wgpu; "work in progress" | 2026-07 | ? | Pure Rust (rustybuzz) [read] |
| `subrandr` 0.0.0 | SRV3 and WebVTT renderer, C API only; no regions or vertical text [read] | 2025-08 | ? | Name placeholder [read] |

No crate implements WebVTT's cue-positioning rules [not found]. Blink's `vtt_cue_layout_algorithm.cc` is the nearest reference [read]. Most of these crates appeared in 2025–2026 and have few users [read].

## 3. Precedents

| Who | Who draws | OS caption settings |
|---|---|---|
| Flutter `video_player` | The app, from a subtitle file it supplies. Text and timing only, with a TODO for styles; in-band tracks are not read [read] | No [read] |
| Media3 `PlayerView` | `SubtitleView`, a View drawn with Canvas over the video [read] | Yes: `PlayerView` applies `CaptioningManager`'s style and size by default [read] |
| Chrome | Blink lays out and draws WebVTT cues as page content [read] | Yes: `MACaptionAppearance` on macOS, `ClosedCaptionProperties` on Windows, `CaptioningManager` on Android, all as a `video::cue` style sheet [read] |
| Safari (WebKit) | The page draws; AVPlayer's drawing is turned off with `suppressesPlayerRendering` [read] | Yes: `MACaptionAppearance` becomes `::cue` CSS, and changes are followed [read] |
| mpv | Itself: text subtitles through libass, image subtitles as bitmaps [read] | No: its own `--sub-*` options; the OS is used only for track language [read] |
| VLC | Itself: FreeType text renderer [read], libass for ASS [inferred] | [not found] |

## 4. What an app-drawn design must add

**The caption settings of each OS, and their change events:**

- **Apple (`MACaptionAppearance`).** On, off or automatic; preferred languages and SDH; colours and opacity for text, background and window; corner radius; font; size; edge style. A flag per setting says whether it overrides the content's styling. There are profiles and a change notification [read].
- **Android (`CaptioningManager`).** Enabled, locale, font scale, colours, edge and typeface, and a change listener [read].
- **Windows (`ClosedCaptionProperties`).** Font colour, opacity, size, style and effect; background and region colours; `PropertiesChanged` [read]. The Default theme cannot be detected through this API [read, Chrome].
- **Web.** None [read].

**Other things the system path gives for free:**

- **Telling accessibility what is shown.** On Apple, `MACaptionAppearanceDidDisplayCaptions` tells assistive products which captions are on screen [read]. An equivalent on Android and Windows [not found].
- **Choosing the track.** On Apple, forced-only subtitles are chosen to match the audio [read]. The player does this whoever draws [inferred].
- **Picture-in-picture, AirPlay and external displays.** These show what the system draws; subtitles the program draws do not reach them [inferred].
- **Frames mode.** When the program draws the video frames itself, only Windows still draws subtitles for it. On Apple, Android and the web, frames mode needs A anyway [inferred].

## Sources

- macOS SDK headers: `AVFoundation.framework/Headers/{AVPlayerItemOutput.h, AVMediaFormat.h, AVMediaSelectionGroup.h}`, `CoreMedia.framework/Headers/{CMTextMarkup.h, CMFormatDescription.h}`, `MediaAccessibility.framework/Headers/MACaptionAppearance.h`.
- Chromium (chromium.googlesource.com/chromium/src, read 2026-08-15): `ui/native_theme/caption_style*`, `chrome/browser/chrome_content_browser_client.cc`, `content/browser/accessibility/captioning_controller.cc`, `third_party/blink/renderer/core/css/css_default_style_sheets.cc`, `third_party/blink/renderer/core/html/track/vtt/`, `media/formats/hls/`.
- WebKit on GitHub: `Source/WebCore/html/track/TextTrackCueGeneric.h`, `platform/graphics/avfoundation/objc/MediaPlayerPrivateAVFoundationObjC.mm`, `page/CaptionUserPreferencesMediaAF.cpp`.
- androidx/media on GitHub (`release`): `Cue.java`, `DefaultSubtitleParserFactory.java`, `SubtitleDecoderFactory.java`, `SubtitleView.java`, `PlayerView.java`. Android reference: `CaptioningManager`.
- Microsoft Learn: `TimedMetadataTrackPresentationMode`, `TimedMetadataKind`, `ImageCue`, `TimedTextStyle`, `ClosedCaptionProperties`, `MediaPlayer.RenderSubtitlesToSurface`, "Media items, playlists, and tracks", "Supported codecs", "Adaptive streaming".
- MDN: `TextTrack.mode`, `VTTCue`.
- flutter/packages on GitHub: `video_player/lib/video_player.dart`, `lib/src/web_vtt.dart`.
- mpv `DOCS/man/options.rst`; VLC `modules/text_renderer/`.
- Crates: `cargo search`/`cargo info`, lib.rs and docs.rs pages for each crate above, `github.com/altqx/rassa` (README and crate `Cargo.toml`s), `github.com/wiedymi/ass-rs` (`ass-renderer` Cargo.toml and build.rs), `github.com/afishhh/subrandr`.
