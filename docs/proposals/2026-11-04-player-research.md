# What a robust player reads from its engine: research

Research for the `media-player` interface (`wit/media-players.wit`). 2026-11-04. Reading only; nothing was built or run. The question: what do real, robust open-source players read from, and ask of, the engine that plays the media, so that the interface can carry it? This note reports what the code does. It does not design the interface.

Marks: **[read]** a source file, header or doc was read; **[inferred]** my conclusion from what was read; **[not found]** looked for and not found. A claim with no mark is **[read]**.

## The answer in short

- Every player studied runs on a handful of facts it asks the engine for again and again: where it is, how long the media is, whether it is playing, whether it is waiting for data, whether it can seek, how much is loaded, whether a seek is under way, and why it stopped. The current WaOS state has the first three and the last one only as a string.
- The facts that decide whether a controller looks right are about waiting, not playing. A scrubber with a grey loaded bar, a spinner during a stall, a disabled scrubber on a live stream, and a "paused" button after the system paused the player all need an answer from the engine that the program cannot guess.
- The four players split on three points: how buffered data is told (one end point, or a list of ranges), how a live stream is told (a flag, or only by missing duration and a moving seekable window), and how a seek is asked for (exact or fast, and when it is done). The web, the weakest system player, sets the floor on many of these.
- Most extras are not the engine's work. Scrub thumbnails, A-B loop, chapters-by-title in the UI, resume position, picture-in-picture windows and statistics screens are built by the players above the engine, from position, seek and a second reader.
- The current `playing` is defined as "from `play` until `pause`, or the end". Media3 and AVPlayer both show that the system pauses a player on its own (headphones unplugged, another app taking audio), and the controller must show that. This is the least obvious gap.

## 1. What each player reads

### IINA (macOS, on libmpv)

IINA keeps its own copy of the state, `PlaybackInfo`, filled from mpv in two ways [read]:

- **Property observers.** `MPVController.observeProperties` lists what mpv pushes: `pause`, `speed`, `mute`, `volume`, `audio-delay`, `sub-delay`, `chapter`, `loop-file`, `loop-playlist`, `track-list`, `aid/vid/sid`, `media-title`, `idle-active`, `current-ao`, `audio-device-list`, `video-params/primaries`, `video-params/gamma`. Each lands in a handler in `handlePropertyChange` (`iina/MPVController.swift`).
- **A timer that polls.** `PlayerCore.syncUI(.time)` reads `time-pos`, `duration`, `eof-reached`, `time-remaining`, `playtime-remaining`, and for network media `paused-for-cache`, `demuxer-cache-state` (`fw-bytes`, `raw-input-rate`), `demuxer-cache-time` and `cache-buffering-state` (`iina/PlayerCore.swift`). The timer runs every 0.1 s, or 0.04 s when the time display shows milliseconds (`iina/AppData.swift`: `syncTimeInterval`, `syncTimePreciseInterval`). `refreshSyncUITimer` stops it when playback is paused, and keeps it on for network media "since the buffering indicator may need to show at any time". Position is not pushed by mpv to IINA; IINA reads it.
- **Events.** `handleEvent` in `MPVController.swift` handles `START_FILE`, `FILE_LOADED`, `SEEK` (sets `isSeeking = true`), `PLAYBACK_RESTART` (clears it), `END_FILE` (reads only whether the reason was `STOP`), `VIDEO_RECONFIG`, `AUDIO_RECONFIG`, `COMMAND_REPLY`, `QUEUE_OVERFLOW`.
- **What the screen shows from this.** A seek bar with chapter ticks and the loaded end (`iina/PlaySliderCell.swift` `drawBar`: chapters from `info.chapters`, loaded end from `info.cacheTime`, network media only). A "Buffering… N%" or "Seeking…" throbber (`iina/BufferIndicatorView.swift` `updateNetworkState`, using `pausedForCache`, `bufferingState`, `isSeeking`, `cacheUsed`, `cacheSpeed`). A statistics window with file size, video and audio bitrate, A/V sync and dropped frames (`iina/InspectorWindowController.swift`: `file-size`, `video-bitrate`, `audio-bitrate`, `avsync`, `frame-drop-count`, `estimated-vf-fps`).
- **System Now Playing.** `iina/NowPlayingInfoManager.swift` `updateInfo` fills `MPNowPlayingInfoCenter` with title, album, artist, media type, asset URL, queue count and index, chapter count and number, duration, elapsed time, playback rate (zero while paused) and default rate (1), plus artwork. Remote commands play, pause, toggle, stop, next, previous, skip forward and back, change repeat mode, change playback rate, change playback position are handled in `AppDelegate.swift` (class using `MPRemoteCommandCenter`, `remoteCommand.*Command.addTarget`). IINA tells mpv not to take the media keys (`MPVOption.Input.inputMediaKeys` set to no in `MPVController`).

### mpv (the engine behind IINA, and its own on-screen controller)

mpv's `osc.lua` is a full controller written only against mpv's public properties [read, `player/lua/osc.lua`]:

- **Observed.** `pause`, `volume`, `mute`, `duration`, `chapter-list`, `track-list`, `playlist-count`, `playlist-pos-1`, `demuxer-cache-state`, `playback-time`, `idle-active`, `vo-configured`, `display-fps`, window state.
- **Read when drawing.** `percent-pos` (slider position, and "enabled" when it is not nil), `paused-for-cache` (the play button shows a clock while it is true or unknown: `mp.get_property_bool("paused-for-cache") ~= false`), `play-direction`, `time-remaining` or `playtime-remaining`, `loop-playlist`, `video-out-params` (for the thumbnail's size).
- **Loaded ranges.** The slider's `seekRangesF` draws each entry of `demuxer-cache-state["seekable-ranges"]` (`start`, `end`) as a band; the "cache" button shows `cache-duration` (`osc.lua`, `cache_enabled`, `seekRangesF`).
- **Seeking.** Press on the bar: `seek <pct> absolute-percent+exact`. Drag: `absolute-percent`, with `+exact` unless the user option `seekbarkeyframes` is on, and repeated identical seeks are dropped (`osc.lua`, seekbar `eventresponder`).
- **Thumbnails** are not mpv's. The bar publishes the hover time to `user-data/osc/hover-sec` and a rectangle to `user-data/osc/draw-preview`, for a separate script to answer (`osc.lua`, `draw_thumbnail` branch).
- **Statistics** (`player/lua/stats.lua`) read `demuxer-cache-state`, `demuxer-cache-duration`, `decoder-frame-drop-count`, `frame-drop-count`, `hwdec-current`, `video-params`, `audio-params`, `audio-delay`, `current-tracks/*`.

mpv's own client API (`include/mpv/client.h`) adds the contract: `MPV_EVENT_SEEK` ("Playback stops"), `MPV_EVENT_PLAYBACK_RESTART` ("The main purpose is allowing the client to detect when a seek request is finished"), and `END_FILE` with a reason (`EOF`, `STOP`, `QUIT`, `ERROR`, `REDIRECT`) and, for `ERROR`, an error code (`LOADING_FAILED`, `AO_INIT_FAILED`, `VO_INIT_FAILED`, `NOTHING_TO_PLAY`, `UNKNOWN_FORMAT`, `UNSUPPORTED`, `GENERIC`).

### VLC 3.0.x

- **libvlc** gives a thin slice (`include/vlc/libvlc_media_player.h`, `libvlc_events.h`; the wiring in `lib/media_player.c`, branch 3.0.x, which is not in the shallow clone and was fetched as a raw file). State is one enum: `NothingSpecial, Opening, Buffering (deprecated), Playing, Paused, Stopped, Ended, Error`. Events: `Buffering` (a float percent, `new_cache`), `Playing`, `Paused`, `EndReached`, `EncounteredError` (no reason), `TimeChanged`, `PositionChanged`, `LengthChanged`, `SeekableChanged`, `PausableChanged`, `Corked` and `Uncorked`, `Muted`, `AudioVolume`, `AudioDevice`, `ChapterChanged`, elementary-stream added, deleted, selected. `input_event_changed` only sends position and time while the state is `PLAYING_S` ("Don't send the position while stopped").
- **Seeking** is `libvlc_media_player_set_time` (milliseconds) or `set_position` (a fraction 0.0 to 1.0). Neither has a precision argument in 3.0 ("Not all formats and protocols support this"). Rate: `libvlc_media_player_set_rate`, whose doc warns "the requested rate may be different from the real playback rate" and "even then, it might not actually work".
- **Statistics** are `libvlc_media_stats_t` (`i_lost_pictures`, `i_displayed_pictures`, `i_lost_abuffers`, `f_input_bitrate`, `i_demux_corrupted`, and so on).
- **VLC's own Qt controller** does not use libvlc; it reads the core's input variables (`modules/gui/qt/input_manager.cpp`): `length`, `time`, `position` (pushed on `INPUT_EVENT_POSITION`), `state`, `rate`, `can-seek`, `can-rewind`, `can-record`, `cache` (pushed on `INPUT_EVENT_CACHE`, shown by `SeekSlider::updateBuffering`), `audio-delay`, `spu-delay`, titles and chapters, and statistics. `SeekSlider` (`util/input_slider.cpp`) disables itself when the input is not seekable (`b_seekable`), throttles seeks while dragging with a 150 ms timer (`seekLimitTimer`), and shows the buffering state only after it has lasted more than a second (`bufferingStart.addSecs(1)`).

### Media3 (Android)

`PlayerControlView` and `DefaultTimeBar` (`libraries/ui/src/main/java/androidx/media3/ui/`) are the cleanest statement of what a controller needs from a `Player` [read]:

- **What it reads each time it draws** (`updateProgress`): `getContentPosition`, `getContentBufferedPosition`, `getContentDuration` (or the timeline's window durations), `isPlaying`, `getPlaybackState`, `getPlaybackParameters().speed`. The buffered position is one number, not ranges. Position is polled on a self-scheduled timer: the delay is the media time one pixel of the bar stands for, capped to the next full second, divided by the playback speed, and clamped between `timeBarMinUpdateIntervalMs` and a maximum. It stops polling when playback is not `isPlaying` and the state is `ENDED` or `IDLE`.
- **What it asks about what it may do** (`updateNavigation`, and `isCommandAvailable` throughout): `COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM`, `SEEK_BACK`, `SEEK_FORWARD`, `SEEK_TO_NEXT`, `SEEK_TO_PREVIOUS`, `SET_SPEED_AND_PITCH`, `SET_REPEAT_MODE`, `GET_TRACKS`, `PLAY_PAUSE`, `PREPARE`. A button is greyed out where the command is not available. The player says what it can do; the controller does not infer it.
- **Play button logic** (`Util.shouldShowPlayButton`): show play when `!getPlayWhenReady()`, or the state is `IDLE` or `ENDED`, or playback is suppressed for any reason except scrubbing. `getPlayWhenReady` is the app's intent; `isPlaying` is "intent, and ready, and not suppressed". Suppression reasons are transient audio-focus loss, unsuitable audio output, and scrubbing (`Player.PLAYBACK_SUPPRESSION_REASON_*`). `onPlayWhenReadyChanged` carries a reason: user request, audio-focus loss, audio becoming noisy (headphones out), remote, end of item, suppressed too long (`PLAY_WHEN_READY_CHANGE_REASON_*`, `Player.java`).
- **Scrubbing.** While the user drags, the control view turns on `ExoPlayer.setScrubbingModeEnabled(true)` by reflection (the interface is ExoPlayer's, not `Player`'s), seeks on each move, and hides the loaded bar (`setBufferedPosition(... ? position : bufferedPosition)`). `ScrubbingModeParameters` (fetched from `libraries/exoplayer/.../ScrubbingModeParameters.java`) holds seek tolerances, and `SeekParameters` holds `EXACT`, `CLOSEST_SYNC`, `PREVIOUS_SYNC`, `NEXT_SYNC` (default `EXACT`).
- **Waiting and errors** in `PlayerView`: a spinner while `getPlaybackState() == STATE_BUFFERING` (and optionally only if `getPlayWhenReady()`), and an error text from `getPlayerError()` through an app-supplied `ErrorMessageProvider` (`PlayerView.java`, lines around `showBufferingSpinner`). `PlaybackException` carries a code from about sixty (`ERROR_CODE_IO_NETWORK_CONNECTION_FAILED`, `..._BAD_HTTP_STATUS`, `..._FILE_NOT_FOUND`, `PARSING_CONTAINER_MALFORMED`, `DECODER_INIT_FAILED`, `DECODING_FORMAT_UNSUPPORTED`, `AUDIO_TRACK_INIT_FAILED`, `DRM_*`, `BEHIND_LIVE_WINDOW`, `TIMEOUT`).
- **Live** is told by `Player.isCurrentMediaItemLive()`, `getCurrentLiveOffset()` (offset from the live edge), and the timeline window's `isSeekable`, `isDynamic` and `windowStartTimeMs` (`Player.java`, `Timeline.java`). `ERROR_CODE_BEHIND_LIVE_WINDOW` is the "fell off the DVR window" error. The engine itself nudges speed to hold the live edge (`LivePlaybackSpeedControl`), so the speed the app asked for is not the speed it gets.

## 2. The needs, one by one

Each entry: what the players do, what breaks without it, where, and what the system players give. System players abbreviate as **Apple** (AVPlayer, AVPlayerItem), **Android** (Media3 ExoPlayer), **Windows** (`Windows.Media.Playback.MediaPlayer`, `MediaPlaybackSession`), **Web** (`HTMLMediaElement`).

### 2.1 Position, read when needed

- **Does.** IINA reads `time-pos` on a 0.1 s timer; Media3's controller reads `getContentPosition` on a timer it sets from the bar width and speed; mpv's OSC observes `playback-time` and redraws; VLC's Qt pushes `time` and `position` on `INPUT_EVENT_POSITION`.
- **Breaks without.** A bar that moves in steps of the event rate (Web's `timeupdate` fires "between about 4 Hz and 66 Hz", `mdn htmlmediaelement/timeupdate_event`).
- **Quirk found.** At the end, mpv leaves `time-pos` at the last frame; IINA sets the position to the duration when `eof-reached` (`iina/PlayerCore.swift` `syncPosition`, with its comment). Time labels at the end then read "00:59" of "01:00".
- **System.** Apple `AVPlayer.currentTime()`; Android `Player.getCurrentPosition`; Windows `MediaPlaybackSession.Position`; Web `currentTime`. This is already decided and is here only to put the others in context.

### 2.2 Waiting for data (buffering) apart from playing

- **Does.** mpv `paused-for-cache`; IINA shows a throbber from it (`iina/BufferIndicatorView.swift`); mpv's OSC swaps the play icon for a clock; VLC `cache` variable and `Buffering` event; Media3 `STATE_BUFFERING`.
- **Breaks without.** A frozen picture with a "pause" button and a moving clock that does not move. A user cannot tell a stall from a hang.
- **Debounce.** VLC's slider shows buffering only after one second (`input_slider.cpp`). Apple says not to show waiting UI while the reason is "still evaluating" (`AVPlayer.h`, `AVPlayerWaitingWhileEvaluatingBufferingRateReason`: "Recommended practice is not to show UI indicating the waiting state"). A flag that flips for a few frames needs a debounce in the UI.
- **Percent is not portable.** mpv `cache-buffering-state` (0 to 100, valid only while paused for cache), VLC `new_cache` and Windows `BufferingProgress` (0 to 1) give a percent; Apple (`timeControlStatus` plus `reasonForWaitingToPlay`), Media3 (`STATE_BUFFERING`) and Web (`waiting` event, `readyState`) give a flag only [read].
- **System.** Apple `AVPlayer.timeControlStatus == WaitingToPlayAtSpecifiedRate`, `reasonForWaitingToPlay`, and item `isPlaybackBufferEmpty`, `isPlaybackLikelyToKeepUp`, `AVPlayerItemPlaybackStalled`; Android `Player.getPlaybackState() == STATE_BUFFERING`, `isLoading`; Windows `MediaPlaybackState.Buffering`, `BufferingProgress`; Web `waiting` and `stalled` events, `readyState`.
- **WaOS now.** Being decided (per the brief).

### 2.3 Loaded ranges (the grey part of the bar)

- **Does.** mpv draws every `seekable-ranges` entry (`osc.lua` `seekRangesF`); IINA draws one end, `demuxer-cache-time` (`PlaySliderCell.drawBar`); Media3 draws one end, `getContentBufferedPosition`; VLC's slider has no loaded bar, only a buffering percent.
- **Breaks without.** The user cannot tell where a click will land instantly and where it will make the player wait. After a seek past the loaded end the bar looks like a lie.
- **Ranges or one end.** One end is enough when the player downloads forward only (Media3, Windows `DownloadProgress`: "buffered between the current playback position and the position provided by this property"). Several ranges are needed once a player keeps data behind the position or after a seek (mpv: "ranges will overlap for a bit, before being joined"). Apple and Web give ranges.
- **System.** Apple `AVPlayerItem.loadedTimeRanges`; Android `getBufferedPosition`, `getTotalBufferedDuration`; Windows `MediaPlaybackSession.GetBufferedRanges()`, `BufferedRangesChanged` (both in the version table of `mediaplaybacksession.md`), `DownloadProgress`; Web `buffered` (`TimeRanges`).

### 2.4 Seekable: whether and where

- **Does.** mpv's OSC disables the seek bar when `percent-pos` is nil; its `seekable` and `partially-seekable` properties exist ("seekable only because the cache is active... larger seeks may fail anyway"). VLC's Qt reads `can-seek` and disables the slider. Media3 asks `isCommandAvailable(COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)` and the window's `isSeekable`.
- **Breaks without.** A scrubber that does nothing on live streams, on pipes, and on servers that refuse range requests.
- **Seekable ranges differ from loaded ranges.** On a live stream the seekable range is the DVR window (see 2.9). On a progressive file the seekable range is the whole file.
- **System.** Apple `AVPlayerItem.seekableTimeRanges` (a seek outside "will be cancelled and the completion handler will be invoked with finished NO"); Android `Timeline.Window.isSeekable`, `isCurrentMediaItemSeekable`, `COMMAND_SEEK_*`; Windows `MediaPlaybackSession.CanSeek`, `GetSeekableRanges()`, `SeekableRangesChanged`; Web `seekable`.

### 2.5 A seek in progress, and when it is done

- **Does.** IINA sets `isSeeking` on `MPV_EVENT_SEEK` and clears it on `MPV_EVENT_PLAYBACK_RESTART`, shows "Seeking…" while it is set, and times the gap between the two to decide whether future seeks may be exact (`MPVController.handleEvent`; `PlayerCore.seek(relativeSecond:option:)` with `useExactSeekForCurrentFile = interval < 0.05`). The client API says the restart event exists "allowing the client to detect when a seek request is finished" (`client.h`). VLC's slider lets only the last seek through after a drag (`seekLimitTimer`); mpv's bar drops duplicate seeks (`lastseek`); Media3's controller keeps its own position text while `scrubbing`.
- **Breaks without.** While a seek is in flight the position still reads the old value, so a scrubber snaps back and then forward. Without a done signal a program cannot tell the end of a drag from the arrival of the frame, cannot count the cost of a seek, and cannot make thumbnails with a second player (2.18).
- **The current text.** `seek` says "its state tells the position once it is there"; nothing says when that is.
- **System.** Apple `seekToTime:toleranceBefore:toleranceAfter:completionHandler:` (the `finished` flag is false when a later seek replaced it), `AVPlayerItemTimeJumpedNotification`, `cancelPendingSeeks`; Android `Player.Listener.onPositionDiscontinuity` with `DISCONTINUITY_REASON_SEEK` (`Player.java`; an `onSeekProcessed` callback is [not found] in the current `Player`); Windows `MediaPlaybackSession.SeekCompleted` ("Occurs when a seek operation... completes", `seekcompleted.md`); Web `seeking`, `seeked`, `HTMLMediaElement.seeking`.

### 2.6 Exact versus fast seeks

- **Does.** mpv has both and defaults by kind: `keyframes` for relative, percent seeks, `exact` for absolute (`input.rst`, `seek`). IINA exposes a preference with `auto` (above). Media3 has `SeekParameters` (`EXACT`, `CLOSEST_SYNC`, `PREVIOUS_SYNC`, `NEXT_SYNC`, default `EXACT`) and a scrubbing mode that loosens the tolerance while the user drags. VLC 3.0 has neither (`set_time` and `set_position` have no flag) [read, `libvlc_media_player.h`].
- **Breaks without.** Exact-only: dragging a bar on long-GOP video is slow, a choppy preview. Fast-only: a click on a subtitle line or a chapter lands seconds early.
- **System.** Apple tolerances on `seek(to:toleranceBefore:toleranceAfter:)` (zero means "sample accurate seeking which may incur additional decoding delay"); Android `ExoPlayer.setSeekParameters`; Windows [not found: a precision option on `Position`]; Web `fastSeek()` exists (listed on the MDN index) beside the exact `currentTime`.

### 2.7 Playback speed: set, limits, and what you actually get

- **Does.** IINA clamps to mpv's minimum 0.01 (`AppData.mpvMinPlaybackSpeed`), mpv accepts 0.01 to 100 (`options.rst`, `--speed`). Media3's menu offers `0.25, 0.5, 0.75, 1, 1.25, 1.5, 2` (`PlayerControlView.PLAYBACK_SPEEDS`). IINA's Now Playing sends rate zero while paused and the default rate 1 (`NowPlayingInfoManager`).
- **Effective rate differs from the asked rate.** Apple: "The effective rate of playback may differ from the desired rate... if the processing algorithm... requires quantization" and "AVPlayer can reset the desired rate to 0.0 when a change in overall state requires playback to be halted" (`AVPlayer.h`, comments on `rate`). VLC: "the requested rate may be different from the real playback rate". Media3's live control changes speed itself.
- **Supported rates differ by media.** Apple `canPlaySlowForward`, `canPlayFastForward`, `canPlayReverse`, `canPlayFastReverse`; Windows `IsSupportedPlaybackRateRange`, `SupportedPlaybackRatesChanged`, and an exception for rates above 1 when an item cannot be skipped (`mediaplaybacksession_playbackrate.md`); Web: browsers mute outside a range (Gecko 0.25 to 4.0, `htmlmediaelement/playbackrate`), negative rates "not yet widespread".
- **Breaks without.** A speed menu offering what the media cannot do; a Now Playing session that says 2x while the player plays 1x.
- **WaOS now.** The state is being given a speed (per the brief). Not in the interface: which rates work.

### 2.8 Volume and mute

- **Does.** IINA/mpv `volume` (percent, up to `volume-max`) and `mute` as separate observed properties; VLC `libvlc_audio_set_volume` (percent, may exceed 100) and `libvlc_audio_set_mute` with `Muted` and `Unmuted` events. IINA hides the mute and volume controls when there is no audio track (`PlayerCore.aidChanged`).
- **Breaks without.** Mute as "volume 0" loses the level to restore. This is cheap to hold in the program, so the loss is small. Media3 itself works this way: `Player.mute()` "sets the audio volume to 0" and `unmute()` picks a non-zero value "decided by the Player" (`Player.java`).
- **Read-back.** Media3 has `onVolumeChanged` and `onDeviceVolumeChanged(volume, muted)`; VLC sends `AudioVolume`.
- **System.** Apple `AVPlayer.volume`, `AVPlayer.isMuted`; Android `Player.setVolume` (0 to 1, no separate mute) and, apart from the player, device volume and mute (`COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS`, `isDeviceMuted`); Windows `Volume`, `IsMuted`, `IsMutedChanged`; Web `volume`, `muted`.
- **Gain over 100 percent** (mpv 130, VLC to 200) is not in any system player's range (Media3 `setVolume` is 0 to 1, `Player.java`; Apple, Windows and Web: 0 to 1 [from memory, not re-read]).

### 2.9 Live streams

- **Does.** Media3 is the only one that says it in the API: `isCurrentMediaItemLive`, `getCurrentLiveOffset`, window `isSeekable` and `isDynamic`. The control view treats a window with no known duration as live. mpv has **no** live property [not found in `input.rst`]: a live stream is a file with no `duration` and a `seekable` that is false or narrow [inferred]. VLC [not found]; the Qt bar relies on `can-seek`.
- **What a live UI needs.** A "live" badge; a bar that spans the seekable window, not the duration; a "go to live" button; "behind live by N s". Without a live flag a podcast with unknown duration looks like a live stream and vice versa.
- **System.** Apple has no boolean; duration stays `kCMTimeIndefinite` until known (`AVPlayerItem.h`, `duration`), and `seekableTimeRanges` is the window; `currentDate()` maps the position to a real date; `configuredTimeOffsetFromLive`, `recommendedTimeOffsetFromLive` set and suggest the offset; `canUseNetworkResourcesForLiveStreamingWhilePaused` trades power for a moving window. Android: above. Windows: `RealTimePlayback` is an input switch ("configures the MediaPlayer for real-time scenarios such as live streaming video"), not a state; `GetSeekableRanges` [read as named]. Web: `duration` is `Infinity` for live (`htmlmediaelement/duration`) and `getStartDate()` gives the wall-clock start.
- **WaOS now.** `duration` is none "for a live stream" but none also means "not yet known". No seekable window, no live flag, no live edge.

### 2.10 End, loop, and why it stopped

- **Does.** mpv `END_FILE` reason; IINA ignores everything but `STOP` and treats an `END_FILE` during loading as a failed open (`PlayerCore.fileEnded`, `idleActiveChanged`: it closes the window and shows an alert). `resume()` after the end seeks to zero first (`PlayerCore.resume`, using `eof-reached`). Media3 `STATE_ENDED`; Apple `AVPlayerItemDidPlayToEndTime`.
- **Ending is not always the end.** The mpv header says `EOF` "may also happen on incomplete or corrupted files, or if the network connection was interrupted when playing a remote file". Apple posts a different notice for that case, `AVPlayerItemFailedToPlayToEndTime` (with an `NSError`). A player that reports only `ended` would show a dropped connection as "finished" and skip to the next track.
- **After the end the picture stays** and play restarts from zero in every player read (`keep-open`-style behaviour; Apple's default `actionAtItemEnd` is pause, `AVPlayer.h`).
- **Looping.** mpv `loop-file` takes a count or `inf`; Media3 `RepeatMode` off, one, all; Windows `IsLoopingEnabled`; Web `loop`; Apple `AVPlayerLooper` [header file listed, not read]. A count is easy for a program to keep above a loop switch.
- **WaOS now.** `ended` exists, and the failure case is `failed(string)` in `player-status`. The doc is silent on a player that was ready and then failed.

### 2.11 Errors: kind, moment, and recovery

- **Kinds in use.** Media3 shows an error text and lets the app choose by code (`PlayerView.errorMessageProvider`); `BEHIND_LIVE_WINDOW` is recovered by seeking to the default position and preparing again [inferred from the code's naming; the recovery itself is in documentation I did not read]. mpv's ending codes map to "could not open", "unknown format", "nothing to play", "no audio device", "no video output". Windows `MediaPlayerError`: `Aborted`, `NetworkError`, `DecodingError`, `SourceNotSupported`, `Unknown`. Web `MediaError.code`: `MEDIA_ERR_ABORTED`, `NETWORK`, `DECODE`, `SRC_NOT_SUPPORTED`. VLC: no kind at all (`EncounteredError`, with the message through `libvlc_errmsg`).
- **What an app does with a kind.** Retry on network, say "unsupported file" on format, say "no sound device" on audio-init, and treat a not-found file differently from a refused network address. IINA does none of it and shows one alert (`Utility.showAlert("error_open")`), which is what a missing kind gives.
- **Moment.** Failure while opening and failure while playing are two events on Apple (`AVPlayerItem.status == Failed` versus `AVPlayerItemFailedToPlayToEndTime`) and on mpv (error at load versus `END_FILE` error).
- **Recovery.** An Apple item "can no longer be used for playback" after a failure (`AVPlayerItem.h`, `status`), so recovery is to open a new one; the WaOS `open` already has that shape.
- **WaOS now.** `failed(string)` is a sentence for the user, not a kind a program can act on.

### 2.12 The system pauses the player

- **Does.** Media3 reports the reason (`onPlayWhenReadyChanged` with `AUDIO_FOCUS_LOSS`, `AUDIO_BECOMING_NOISY`; `onPlaybackSuppressionReasonChanged` with transient audio-focus loss and unsuitable audio output). Apple resets `rate` to zero "when an interruption occurs on iOS, as announced by AVAudioSession" (`AVPlayer.h`). VLC sends `Corked` and `Uncorked` (`media_player.c` `corks_changed`; what causes it [inferred: the audio server pausing the stream]). Windows has `AudioStateMonitor` (named in the version table of `mediaplayer.md`; not read). Web: `pause` event fires for system pauses [not read].
- **Breaks without.** The user pulls out headphones, the system pauses, and the program's button still says "pause" because `playing` stayed true. Or the program restarts a call-interrupted video by itself.
- **The current text.** `playing` is "from `play` until `pause`, or the end". The wording rules out a system pause. Inset's camera session already has `interrupted(option<camera-interruption>)` for the same kind of fact (`wit/cameras.wit`).
- **System.** As listed.

### 2.13 Chapters

- **Does.** mpv `chapter-list` (title, time), `chapter`, `chapters`; IINA draws ticks and prev/next chapter, and seeks to `chapter.time` (`PlayerCore`), publishes chapter count and number to Now Playing; mpv's OSC draws markers, shows the chapter title on hover, makes the prev/next buttons enabled by `#chapter_list > 0`. VLC `libvlc_media_player_get_chapter`, `ChapterChanged`, `next_chapter`. Media3's controller has no chapters [not found in `PlayerControlView`; chapters appear only as ad markers].
- **Breaks without.** No ticks, no skip to the next chapter, no titles on hover. These are display features; the data is a list of (start, title).
- **System.** Apple `AVAsset.chapterMetadataGroupsBestMatchingPreferredLanguages:` (`AVAsset.h`), not on the item or the player; Android [not found]; Windows [not found in what was read]; Web: text tracks of kind chapters [inferred, not read].
- **Settled?** The brief puts "track lists and selection" out of scope; chapters are neither in nor out. They are a list in the media, like tracks.

### 2.14 Frame stepping

- **Does.** mpv `frame-step` and `frame-back-step` (the back step "does not work with audio-only"); IINA `frameStep(backwards:)` calls them and turns the display link on first. VLC `libvlc_media_player_next_frame`, forward only.
- **System.** Apple `AVPlayerItem.stepByCount:`, with `canStepForward` and `canStepBackward`; Windows `StepForwardOneFrame`, `StepBackwardOneFrame` (named in `mediaplayer.md`); Android [not found in `Player`]; Web [not found: no API; programs seek by a frame duration, which is not frame-exact].
- **Breaks without.** An editor or analysis program cannot step by frame. A viewer does not need it.

### 2.15 A-B loop

- **Does.** mpv `ab-loop-a`, `ab-loop-b`, `ab-loop-count`; IINA wraps them (`PlayerCore.abLoopA/abLoopB`, `syncAbLoop`). The loop is done inside the engine, so it is exact.
- **System.** Apple `forwardPlaybackEndTime` and `reversePlaybackEndTime` (`AVPlayerItem.h`) with a seek at the end; Android [not found: clipping lives in the media source, not in `Player`]; Windows [not found]; Web [not found]. A program does it with position reads and `seek` and is as exact as the position cadence allows (2.1).

### 2.16 Audio delay (sync) and subtitle delay

- **Does.** mpv `audio-delay`, `sub-delay`; IINA observes and shows both; VLC `libvlc_audio_set_delay` ("reset to zero each time the media changes") and `libvlc_video_set_spu_delay`.
- **System.** [not found] on Apple's `AVPlayer`, Media3's `Player`, Windows' `MediaPlayer`, Web. Subtitle delay is a program's, since it draws the cues (settled).
- **Breaks without.** A bluetooth speaker out of sync cannot be corrected. Only the engine can shift audio.

### 2.17 Playlists and gapless

- **Does.** mpv has a playlist (`playlist`, `playlist-pos-1`, `playlist-count`) and IINA's Now Playing sends queue count and index; mpv's OSC enables next and previous from the playlist position. mpv `--gapless-audio` (default `weak`) keeps the device open across files.
- **Breaks without.** Between tracks of an album there is a click or silence if the program opens the next player only after the end. A second player opened ahead of time cannot be joined to the first without a gap on any system.
- **System.** Apple `AVQueuePlayer` (named in `AVPlayer.h` for `AVPlayerActionAtItemEndAdvance`); Android playlist of `MediaItem`s with a `Timeline` (`Player.COMMAND_SEEK_TO_NEXT`); Windows `MediaPlaybackList` [named from memory, not read]; Web [not found: one `src` per element; MSE joins segments].
- **A program can build** next/previous, shuffle, repeat, a queue UI, and "play next" above one player. It cannot build the gapless join.

### 2.18 Scrub thumbnails

- **Does.** mpv's OSC delegates to a script. IINA makes its own with FFmpeg, cached (`PlayerCore.generateThumbnails`, `FFmpegController`, `ThumbnailCache`). Media3's `PlayerControlView` has none [not found].
- **System.** Apple `AVAssetImageGenerator` "provides thumbnail or preview images of assets independently of playback" (`AVAssetImageGenerator.h`). Android, Windows, Web [not found for the player].
- **Where it lives.** Above the player. A program can make a thumbnail with a second, muted player on the same source, a frame reader, and a seek whose end it can detect (2.5).

### 2.19 System Now Playing and media keys

- **Does.** See IINA above. Media3 does this with a `MediaSession` that wraps the `Player`. mpv leaves the keys to the app. On Windows the player has a `CommandManager` that "specifies the behavior of and receives events from the System Media Transport Controls" (`mediaplayer_commandmanager.md`). On the web, `navigator.mediaSession` (`metadata`, `playbackState`, `setActionHandler`, `setPositionState`) is separate from the element (`mdn mediasession/index`).
- **What goes out.** Title, artist, album, artwork, duration, elapsed position, rate (zero when paused), queue index and count, chapter number. IINA found the system shows a zero position if the rate is not zero while paused (comment in `updateInfo`).
- **What comes in.** Play, pause, toggle, stop, next, previous, skip by seconds, seek to a position, change rate, change repeat mode. These are a stream of requests from the system, not state.
- **WaOS now.** No now-playing or media-key surface in `wit/` or `docs/` [read: searched for now-playing, media-session, remote-command, media-key]. The player's `metadata` is the title and artwork; state has position and rate being added.

### 2.20 Picture-in-picture and AirPlay or casting

- **Does.** IINA puts its own video view in a private `PIPViewController` (`MainWindowController` `pip.presentAsPicture`) and hides the window; the engine knows nothing. Apple's `AVPlayer` has `allowsExternalPlayback` and `isExternalPlaybackActive` (`AVPlayer.h`). Web has `requestPictureInPicture` (`mdn htmlvideoelement/requestpictureinpicture`).
- **Where.** It follows the picture, which is out of scope here. [inferred] Apple's system PiP wants an `AVPlayerLayer`; whether the composition layer can satisfy that is a question for the layer design.

### 2.21 Audio output device

- **Does.** mpv `audio-device-list`, `audio-device`, `current-ao`; IINA watches the list and moves to "auto" when the chosen device disappears (`PlayerCore.audioDeviceListChanged`); VLC `libvlc_audio_output_device_enum`, `AudioDevice` event.
- **System.** Apple `AVPlayer.audioOutputDeviceUniqueID` (macOS only); Windows `MediaPlayer.AudioDevice`; Web `HTMLMediaElement.setSinkId`; Android [not read].
- **WaOS.** The Web Audio design picks the output through `selectAudioOutput` and `set-sink-id` on a context (`docs/proposals/2026-10-29-sound.md`, decision 2). A player has no such choice; its sound goes where the system sends it.

### 2.22 HTTP headers, cookies, user agent

- **Does.** mpv `--http-header-fields`, `--cookies`, `--cookies-file`, `--user-agent`, `--referrer`, `--tls-verify`, `--network-timeout`; IINA lets users set `user-agent`, `referrer`, `network-timeout` and `tls-verify` (`AppData.swift` allow-list). VLC `libvlc_media_add_option`, `libvlc_set_user_agent`.
- **System.** Apple: `AVURLAssetHTTPCookiesKey` and `AVURLAssetHTTPUserAgentKey` are public (`AVAsset.h`); a header-fields key is **[not found]** in the SDK's AVFoundation headers. Windows, Web, Android [not found/not read]: a `<video src>` takes no headers and its credentials follow `crossOrigin`.
- **Consequence.** Headers are not a portable field. A program that needs them hands the player bytes it fetched itself, which Inset has marked "for later" (`media.rs`, `Players::open` doc).

### 2.23 Bitrate, codec and dropped-frame statistics

- **Does.** Only diagnostic screens: IINA's Inspector, mpv's `stats.lua`, VLC's statistics window (`INPUT_EVENT_STATISTICS`). None of the controllers reads them.
- **System.** Apple `AVPlayerItemAccessLogEvent` has `numberOfStalls`, `observedBitrate`, `indicatedBitrate`, `numberOfDroppedVideoFrames` (`AVPlayerItem.h`); Web `getVideoPlaybackQuality()` (`droppedVideoFrames`, `totalVideoFrames`); Android [not read, an analytics listener in ExoPlayer]; Windows [not found].
- **Breaks without.** Nothing a viewer notices.

### 2.24 HDR and colour

- **Does.** IINA observes `video-params/primaries` and `video-params/gamma` and re-arms the display layer's extended-range mode (`MPVController`, `refreshEdrMode`). The signal is about the picture, which in WaOS goes through a layer or a frame (out of scope).
- **System.** Apple `AVPlayer.eligibleForHDRPlayback`/`AVPlayerHDRMode` (`AVPlayer.h`). A player-level "this media is HDR" badge would come with the tracks.

## 3. Grouping

### Group 1: needed for a robust basic player

| Need | What the current interface has | Lacks |
|---|---|---|
| Position read when needed | Being decided | - |
| Speed in state | Being decided | - |
| Waiting for data (buffering) | Being decided | - |
| Loaded data: how far (one end at least; ranges where the engine has them) | Nothing | **Lacks** (2.3) |
| Seekable: whether, and the seekable range | Nothing | **Lacks** (2.4) |
| A seek in progress and its end | `seek` returns nothing; "state tells the position once it is there" | **Lacks** (2.5) |
| Why it stopped: ended, versus failed, versus failed mid-playback | `ended`, `failed(string)` | **Lacks** the kind and the mid-playback case (2.10, 2.11) |
| The system pausing the player | `playing` defined as the program's intent | **Contradicts** (2.12) |
| Read-back of what was set (rate, volume, mute) | Nothing in state | **Lacks**; Apple resets the rate itself (2.7, 2.8) |
| Mute | Only `set-volume` | Separate on Apple, Windows, Web, mpv, VLC; Media3 mutes by volume 0. The program can keep the level (2.8) |
| Duration, size, ended, tracks, metadata, loop, volume, play, pause, seek, rate | Present | - |

### Group 2: advanced, but the interface has to carry it

| Need | Why a program cannot build it | System players |
|---|---|---|
| Live: a flag, the seekable window, the offset from the live edge | The program sees only "no duration" (2.9) | Android full; Apple by ranges and offsets; Web by `Infinity` and `seekable`; Windows by `GetSeekableRanges`; mpv and VLC: no flag |
| Seek precision (exact or fast) and scrubbing | Only the engine knows the cost (2.6) | Apple, Android, mpv: yes; Web `fastSeek`; Windows, VLC 3.0: not found |
| Which rates the media supports | A menu of impossible speeds otherwise (2.7) | Apple, Windows; others by failure |
| Frame stepping, both ways | Needs decoder state (2.14) | Apple, Windows, mpv; Android, Web: not found |
| Gapless next item (a queue the player prepares) | A second player cannot join without a gap (2.17) | Apple, Android, mpv, Windows; Web: not found |
| System Now Playing and media keys: out (metadata, position, rate) and in (commands) | Only the host can talk to the system (2.19) | All four have one, shaped differently |
| Audio delay | Only the engine can shift sound (2.16) | mpv and VLC only |
| Statistics: dropped frames, bitrates | Only the engine counts (2.23) | Apple, Web; others not found |
| Chapters (a list of start and title) | Data in the container (2.13) | Apple on the asset; mpv, VLC; others not found |
| Error kinds | The program cannot read them from a string (2.11) | All four |
| HTTP headers and cookies | Only the system fetches (2.22) | Apple cookies and UA; the web: none; not portable |
| Output device | Sound goes where the system sends it (2.21) | Apple (macOS), Windows, Web |

### Group 3: a program can build it above the interface

- **Scrub thumbnails.** Second muted player, a frame reader, a seek-done signal (2.18). Needs 2.5.
- **A-B loop.** Position reads and `seek`; exact only as far as the position cadence (2.15).
- **Resume position.** Position, duration and the source; save and seek on open. mpv's watch-later (`save-position-on-quit`) is a feature of the app that wraps it.
- **Buffering debounce and spinner delays** (2.2).
- **Playlist, queue, shuffle, repeat-all, loop counts, next and previous** (2.17).
- **Mute** by remembering the level (2.8).
- **Chapter UI, speed menus, time formats, remaining time, tooltips, markers** from data the interface gives.
- **Picture-in-picture windows** where the system lets a window do it, which depends on the layer design (2.20).
- **Subtitle delay** (settled: the program draws cues).
- **Idle-timer power management**: IINA stops its poll timer when paused (`refreshSyncUITimer`). Reading position only when asked makes this free.

## 4. Surprises, and what was not found

- **`playing` as intent is a trap.** The system pauses a player on its own, and Media3 and AVPlayer both have to report it (2.12). Neither maps onto "from `play` until `pause`".
- **`ended` can lie.** mpv says its end-of-file reason also covers an interrupted network connection. Apple has a separate "failed to play to end" notice for exactly that (2.10).
- **No live flag in mpv, VLC, or on Apple.** Media3 is the only player that says "live". The others derive it from a missing duration and a seekable window.
- **The loaded bar is one number in three of four players.** Only mpv draws ranges; IINA, Media3 and the Windows property use one end (2.3).
- **A seek's cost is a measurement.** IINA times the gap between mpv's seek start and restart events to pick exact or fast seeks per file (`PlayerCore.seek`). That needs a completion signal, and it is something no controller could do from position alone.
- **Public Apple headers have no way to set arbitrary HTTP headers** on an `AVURLAsset`; only cookies and the user agent (2.22).
- **Media3 uses reflection to turn on scrubbing** because it is on `ExoPlayer`, not on `Player`. The base interface of the one player that publishes its capabilities (`isCommandAvailable`) still treats scrub mode as an extra (2.5).
- **Not found:** a live flag in mpv or VLC; chapters on Media3's `Player`; frame stepping on Media3 and the Web; audio delay on any system player; a precision flag on VLC 3.0's seek; a Windows header-fields option for `MediaPlayer` (not read).
- **Not read** (so unverified): Android's `setPreferredAudioDevice`, `AnalyticsListener`; Windows' `MediaPlaybackList` and `AudioStateMonitor` contents; Apple's `AVPlayerLooper`; the Web's `pause` behaviour on system pauses, and chapter text tracks.

## Sources

- **IINA** (`iina/iina`, develop, commit 0d3a9c1 in the shallow clone): `iina/MPVController.swift` (`observeProperties`, `handleEvent`, `handlePropertyChange`), `iina/PlayerCore.swift` (`syncUI`, `syncPosition`, `refreshSyncUITimer`, `seek`, `frameStep`, `fileLoaded`, `fileEnded`, `idleActiveChanged`, `resume`, `audioDeviceListChanged`, `abLoopA`), `iina/PlaybackInfo.swift`, `iina/BufferIndicatorView.swift`, `iina/PlaySliderCell.swift`, `iina/InspectorWindowController.swift`, `iina/NowPlayingInfoManager.swift`, `iina/AppDelegate.swift`, `iina/MainWindowController.swift` (PiP), `iina/AppData.swift`, `iina/ThumbnailCache.swift`.
- **mpv** (master, commit fba6342): `player/lua/osc.lua`, `player/lua/stats.lua`, `DOCS/man/input.rst` (`seek`, `frame-step`, `paused-for-cache`, `cache-buffering-state`, `demuxer-cache-state`, `seekable`, `partially-seekable`, `eof-reached`, `core-idle`, `time-pos`, `duration`, `chapter-list`), `DOCS/man/options.rst` (`--speed`, `--hr-seek`, `--keep-open`, `--loop-file`, `--gapless-audio`, `--cache-pause*`, `--http-header-fields`, `--cookies`, `--user-agent`, `--force-seekable`, `--volume-max`), `include/mpv/client.h` (events, end-file reasons, error codes).
- **VLC** (3.0.x, commit c128d1f): `include/vlc/libvlc_media_player.h`, `include/vlc/libvlc_events.h`, `include/vlc/libvlc_media.h` (`libvlc_media_stats_t`, `libvlc_state_t`), `include/vlc/libvlc.h` (`libvlc_set_user_agent`), `modules/gui/qt/input_manager.cpp`, `modules/gui/qt/util/input_slider.cpp`; `lib/media_player.c` fetched from `github.com/videolan/vlc` branch 3.0.x (scratchpad `player-research/vlc-lib/`).
- **Media3** (release, commit 8c6678b): `libraries/ui/.../PlayerControlView.java`, `DefaultTimeBar.java`, `PlayerView.java`, `libraries/common/.../Player.java`, `Timeline.java`, `PlaybackException.java`, `util/Util.java` (`shouldShowPlayButton`); `exoplayer/SeekParameters.java`, `ScrubbingModeParameters.java`, `LivePlaybackSpeedControl.java` fetched from `github.com/androidx/media` (scratchpad `player-research/media3-exo/`).
- **Apple:** macOS SDK headers `AVFoundation.framework/Headers/{AVPlayer.h, AVPlayerItem.h, AVAsset.h, AVAssetImageGenerator.h}`.
- **Windows:** `MicrosoftDocs/winrt-api` (`windows.media.playback/`: `mediaplaybacksession.md`, `mediaplaybackstate.md`, `mediaplayererror.md`, `mediaplaybacksession_bufferingprogress.md`, `..._downloadprogress.md`, `..._playbackrate.md`, `mediaplayer.md`, `mediaplayer_commandmanager.md`, `mediaplayer_realtimeplayback.md`), fetched into the scratchpad `player-research/winrt-docs/`.
- **Web:** MDN (`mdn/content`, `files/en-us/web/api/`: `htmlmediaelement/{index,duration,seekable,playbackrate,timeupdate_event,waiting_event,getstartdate}`, `mediaerror/code`, `htmlvideoelement/getvideoplaybackquality`, `mediasession/index`), fetched into the scratchpad `player-research/mdn/`.
- **WaOS and Inset:** `wit/media-players.wit`, `wit/cameras.wit`, `docs/proposals/2026-11-02-program-media.md`, `docs/proposals/2026-11-02-subtitles-research.md`, `docs/proposals/2026-10-29-sound.md`, `inset-embedder/src/media.rs`.
