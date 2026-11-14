//! `wasdk:platform/media-players`: a program's media, played by the system's player, its picture
//! drawn on a composition layer the program made.
//!
//! The shell plays ([`ShellMediaPlayers`], each player a `ShellMediaPlayer`); the engine keeps the
//! rules and the copies. Where a source is (`media_sources`): a link passes the program's network
//! rules before the shell is handed it, its host looked up first by the program's own lookup, off
//! the program's thread, as decision 8 of the media proposal has it; a path is where its mount puts
//! it on the machine. And each player's state, its media's metadata and its cues as the shell reports
//! them (`player`), which the program reads. The interface is served to a program by `host`.
//!
//! The invariant: nothing the rules refuse reaches the shell, neither a link whose host the
//! program may not reach nor a path outside its files: the player of such a source failed, with
//! why, and no shell's player is opened for it.

pub(crate) mod host;
mod player;

use std::path::Path;
use std::sync::Arc;

use url::Url;

use crate::files::ProgramFiles;
use crate::media_sources::{self, Named};
use crate::network::ProgramNetwork;
use crate::running::RunningProgram;
use crate::shell::ShellMediaPlayers;
pub use player::{MediaPlayer, MediaPlayerReports};

/// `media-players.media-players`: the shell's players, as the program reaches them.
pub struct MediaPlayers(Arc<dyn ShellMediaPlayers>);

impl MediaPlayers {
    /// A player of `source` for `program`: of a path inside `files`, open on the shell at once;
    /// of a link whose host `network` lets the program reach, open once that is known; and of
    /// anything else, failed.
    fn open(
        &self,
        program: &RunningProgram,
        files: &mut ProgramFiles,
        network: &ProgramNetwork,
        source: &str,
    ) -> MediaPlayer {
        match media_sources::named(source) {
            Ok(Named::Path(path)) => self.open_file(program, files, &path),
            Ok(Named::Link(link)) => self.open_once_reachable(program, network, link),
            Err(reason) => MediaPlayer::refused(reason),
        }
    }

    /// A player of the file at the program's `path`, where it is on the machine; failed for a path
    /// outside the program's files.
    fn open_file(
        &self,
        program: &RunningProgram,
        files: &mut ProgramFiles,
        path: &Path,
    ) -> MediaPlayer {
        let url = match media_sources::file_url(files, path) {
            Ok(url) => url,
            Err(reason) => return MediaPlayer::refused(reason),
        };
        let player = MediaPlayer::loading();
        player
            .reports()
            .open_with(|reports| self.0.open(program, &url, reports));
        player
    }

    /// A player of `link`, open on the shell once the program's lookup of its host, made off the
    /// program's thread, lets it reach the host, and failed otherwise.
    fn open_once_reachable(
        &self,
        program: &RunningProgram,
        network: &ProgramNetwork,
        link: Url,
    ) -> MediaPlayer {
        let player = MediaPlayer::loading();
        let reports = player.reports();
        let (shell, program, network) = (Arc::clone(&self.0), program.clone(), network.clone());
        // Detached: a player let go of meanwhile is not opened once the lookup answers.
        drop(tokio::task::spawn_blocking(
            move || match media_sources::check_link(&network, &link) {
                Ok(()) => reports.open_with(|reports| shell.open(&program, link.as_str(), reports)),
                Err(reason) => reports.refused(reason),
            },
        ));
        player
    }
}
