//! The code serving `wasdk:platform/media-players` to a program: the shell's players, where it has
//! them, a player opened on what the program's source names, what it asks of the player, with the
//! shell's own layer or reader it sends the picture to, and the player's state, its cues and its
//! metadata, with their changes.

use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use super::{MediaPlayer, MediaPlayers};
use crate::bindings::wasdk::media::media_players::{
    self, Cue, HostMediaPlayer, HostMediaPlayerWithStore, HostMediaPlayers, MediaMetadata,
    PlayerState, SeekTolerance, VideoOutput,
};
use crate::files::ProgramFiles;
use crate::network::ProgramNetwork;
use crate::running::AppInstance;
use crate::shell::{self, ShellMediaPlayers};
use crate::stream::stream_of;

/// What the media players interface reads and writes in a program's store.
pub(crate) struct MediaPlayersCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a AppInstance,
    /// The shell's players; none where the shell has none.
    pub(crate) media_players: Option<&'a Arc<dyn ShellMediaPlayers>>,
    /// The program's mounts, where a path it names is looked up.
    pub(crate) files: &'a mut ProgramFiles,
    /// What the program may reach of the network, which a link's host is looked up by.
    pub(crate) network: &'a ProgramNetwork,
}

pub(crate) trait MediaPlayersCtxView: Send {
    fn media_players_ctx(&mut self) -> MediaPlayersCtx<'_>;
}

struct HasMediaPlayers;

impl HasData for HasMediaPlayers {
    type Data<'a> = MediaPlayersCtx<'a>;
}

pub(crate) fn add_to_linker<T: MediaPlayersCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    media_players::add_to_linker::<_, HasMediaPlayers>(linker, T::media_players_ctx)
}

impl MediaPlayersCtx<'_> {
    /// `output`, as the shell's player is given it: with the shell's own layer or reader.
    fn shell_output(&self, output: VideoOutput) -> wasmtime::Result<shell::VideoOutput> {
        match output {
            VideoOutput::Layer(layer) => {
                let layer = Arc::clone(self.table.get(&layer)?.shell_layer());
                Ok(shell::VideoOutput::Layer(layer))
            }
            VideoOutput::Frames(reader) => {
                let reader = Arc::clone(self.table.get(&reader)?.shell_reader());
                Ok(shell::VideoOutput::Frames(reader))
            }
        }
    }
}

impl media_players::Host for MediaPlayersCtx<'_> {
    fn get_media_players(&mut self) -> wasmtime::Result<Option<Resource<MediaPlayers>>> {
        let players = self.media_players.cloned().map(MediaPlayers);
        Ok(players
            .map(|players| self.table.push(players))
            .transpose()?)
    }
}

impl HostMediaPlayers for MediaPlayersCtx<'_> {
    fn open(
        &mut self,
        players: Resource<MediaPlayers>,
        source: String,
    ) -> wasmtime::Result<Resource<MediaPlayer>> {
        let player =
            self.table
                .get(&players)?
                .open(self.program, self.files, self.network, &source);
        Ok(self.table.push(player)?)
    }

    fn drop(&mut self, players: Resource<MediaPlayers>) -> wasmtime::Result<()> {
        self.table.delete(players)?;
        Ok(())
    }
}

impl HostMediaPlayer for MediaPlayersCtx<'_> {
    fn set_video_output(
        &mut self,
        player: Resource<MediaPlayer>,
        output: Option<VideoOutput>,
    ) -> wasmtime::Result<()> {
        let output = output.map(|output| self.shell_output(output)).transpose()?;
        self.table.get(&player)?.set_video_output(output);
        Ok(())
    }

    fn play(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<()> {
        self.table.get(&player)?.play();
        Ok(())
    }

    fn pause(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<()> {
        self.table.get(&player)?.pause();
        Ok(())
    }

    fn seek(
        &mut self,
        player: Resource<MediaPlayer>,
        position: u64,
        tolerance: SeekTolerance,
    ) -> wasmtime::Result<()> {
        self.table
            .get(&player)?
            .seek(Duration::from_nanos(position), tolerance);
        Ok(())
    }

    fn set_rate(&mut self, player: Resource<MediaPlayer>, rate: f64) -> wasmtime::Result<()> {
        self.table.get(&player)?.set_rate(rate);
        Ok(())
    }

    fn set_volume(&mut self, player: Resource<MediaPlayer>, volume: f32) -> wasmtime::Result<()> {
        self.table.get(&player)?.set_volume(volume);
        Ok(())
    }

    fn set_muted(&mut self, player: Resource<MediaPlayer>, muted: bool) -> wasmtime::Result<()> {
        self.table.get(&player)?.set_muted(muted);
        Ok(())
    }

    fn set_looping(
        &mut self,
        player: Resource<MediaPlayer>,
        looping: bool,
    ) -> wasmtime::Result<()> {
        self.table.get(&player)?.set_looping(looping);
        Ok(())
    }

    fn state(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<PlayerState> {
        Ok(self.table.get(&player)?.state())
    }

    fn position(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<u64> {
        Ok(nanoseconds(self.table.get(&player)?.position()))
    }

    fn select_audio_track(
        &mut self,
        player: Resource<MediaPlayer>,
        id: String,
    ) -> wasmtime::Result<()> {
        self.table.get(&player)?.select_audio_track(id);
        Ok(())
    }

    fn select_text_track(
        &mut self,
        player: Resource<MediaPlayer>,
        id: Option<String>,
    ) -> wasmtime::Result<()> {
        self.table.get(&player)?.select_text_track(id);
        Ok(())
    }

    fn metadata(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<MediaMetadata> {
        Ok(self.table.get(&player)?.metadata())
    }

    /// The program lets go of the player: the shell's player stops.
    fn drop(&mut self, player: Resource<MediaPlayer>) -> wasmtime::Result<()> {
        self.table.delete(player)?;
        Ok(())
    }
}

impl<T: MediaPlayersCtxView + 'static> HostMediaPlayerWithStore<T> for HasMediaPlayers {
    /// Each state enters the program's stream as it reads it.
    fn state_changes(
        mut access: Access<T, Self>,
        player: Resource<MediaPlayer>,
    ) -> wasmtime::Result<StreamReader<PlayerState>> {
        let changes = access.get().table.get(&player)?.state_changes();
        stream_of(access, changes, |_: &mut T, state| Ok(state))
    }

    /// Each list of cues enters the program's stream as it reads it.
    fn cue_changes(
        mut access: Access<T, Self>,
        player: Resource<MediaPlayer>,
    ) -> wasmtime::Result<StreamReader<Vec<Cue>>> {
        let changes = access.get().table.get(&player)?.cue_changes();
        stream_of(access, changes, |_: &mut T, cues| Ok(cues))
    }

    /// Each metadata enters the program's stream as it reads it.
    fn metadata_changes(
        mut access: Access<T, Self>,
        player: Resource<MediaPlayer>,
    ) -> wasmtime::Result<StreamReader<MediaMetadata>> {
        let changes = access.get().table.get(&player)?.metadata_changes();
        stream_of(access, changes, |_: &mut T, metadata| Ok(metadata))
    }
}

/// `duration` in a WIT duration's nanoseconds; the longest one holds for a longer one.
fn nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
