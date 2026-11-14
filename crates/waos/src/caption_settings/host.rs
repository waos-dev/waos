//! The code serving `wasdk:media/caption-settings` to a program: the user's caption style, and
//! its changes.

use wasmtime::component::{Access, HasData, Linker, Resource, ResourceTable, StreamReader};

use super::CaptionSettings;
use crate::bindings::wasdk::media::caption_settings::{
    self, CaptionStyle, HostCaptionSettings, HostCaptionSettingsWithStore,
};
use crate::stream::stream_of;

/// What the caption settings interface reads in a program's store.
pub(crate) struct CaptionSettingsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    /// The user's caption settings; none where the system keeps none.
    pub(crate) settings: Option<&'a CaptionSettings>,
}

pub(crate) trait CaptionSettingsCtxView: Send {
    fn caption_settings_ctx(&mut self) -> CaptionSettingsCtx<'_>;
}

struct HasCaptionSettings;

impl HasData for HasCaptionSettings {
    type Data<'a> = CaptionSettingsCtx<'a>;
}

pub(crate) fn add_to_linker<T: CaptionSettingsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    caption_settings::add_to_linker::<_, HasCaptionSettings>(linker, T::caption_settings_ctx)
}

impl caption_settings::Host for CaptionSettingsCtx<'_> {
    fn get_caption_settings(&mut self) -> wasmtime::Result<Option<Resource<CaptionSettings>>> {
        let settings = self.settings.cloned();
        Ok(settings
            .map(|settings| self.table.push(settings))
            .transpose()?)
    }
}

impl HostCaptionSettings for CaptionSettingsCtx<'_> {
    fn style(&mut self, settings: Resource<CaptionSettings>) -> wasmtime::Result<CaptionStyle> {
        Ok(self.table.get(&settings)?.style())
    }

    fn drop(&mut self, settings: Resource<CaptionSettings>) -> wasmtime::Result<()> {
        self.table.delete(settings)?;
        Ok(())
    }
}

impl<T: CaptionSettingsCtxView + 'static> HostCaptionSettingsWithStore<T> for HasCaptionSettings {
    /// Each style enters the program's stream as it reads it.
    fn style_changes(
        mut access: Access<T, Self>,
        settings: Resource<CaptionSettings>,
    ) -> wasmtime::Result<StreamReader<CaptionStyle>> {
        let changes = access.get().table.get(&settings)?.style_changes();
        stream_of(access, changes, |_: &mut T, style| Ok(style))
    }
}
