//! A program's settings as `cargo waos settings` reads them and `cargo waos set` sets them: its
//! settings page's rows (`setting_changes`), each by the key `waos-dev-protocol` names it with, a
//! set made as the page makes the same change, so the command and the page are one rule. A set
//! answer is the program's setting, used by every run of it, installed or not (decision 11 of the
//! permissions proposal).
//!
//! The invariant: only a program a run of `cargo waos run` on this machine runs or has run, whose
//! component one sent is among the components kept (`sent_components`), is read or set; any other,
//! one the library installed among them, is `Unreached`.

use std::sync::Arc;

use waos_dev_protocol::{
    ASK, AUDIO_OUTPUT, Answer, Connection, FileDialog, FromWaOS, LINK_SCHEME, MACHINE, Offered,
    Question, Role, Row, SIMULATED_MICROPHONE, ServerStream, SettingRow, SimulatedSound, Switch,
    Unreached,
};
use waos_package::ProgramId;
use waos_store::Store;

use super::environments::ProgramEnvironments;
use super::runs::Runs;
use super::sent_components::SentComponents;
use super::setting_changes::{
    self, SettingChange, Shown, ShownSettings, Which, change_program, program_page,
};

/// What reading and setting a program's settings needs of the app.
pub(crate) struct SettingsContext {
    pub(crate) runs: Arc<Runs>,
    pub(crate) components: Arc<SentComponents>,
    pub(crate) store: Store,
    pub(crate) environments: Arc<ProgramEnvironments>,
}

/// Answers a connection's `Settings`, with `set` none, or its `Set`: the rows of the program
/// `id`'s settings, after the row set where one is.
pub(crate) async fn answer(
    mut connection: Connection<ServerStream>,
    context: &SettingsContext,
    id: &ProgramId,
    set: Option<Row>,
) {
    let answer = rows_after(context, id, set)
        .await
        .unwrap_or_else(|answer| answer);
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// The rows of the program `id`'s settings after `set`, or what refuses it.
async fn rows_after(
    context: &SettingsContext,
    id: &ProgramId,
    set: Option<Row>,
) -> Result<FromWaOS, FromWaOS> {
    if !was_run_by_cargo_waos(context, id).await {
        return Err(FromWaOS::Unreached(Unreached::NeverRunByCargoWaos));
    }
    let environments = Arc::clone(&context.environments);
    // Listing the sound outputs waits for the system's audio.
    let offered = tokio::task::spawn_blocking(move || environments.offered())
        .await
        .map_err(|error| FromWaOS::Refused(format!("reading what the pickers offer: {error}")))?;
    if let Some(row) = set {
        let change = change_of(row, &offered).map_err(FromWaOS::Refused)?;
        tracing::info!(%id, ?change, "cargo waos sets a program's setting");
        set_program(&context.store, id, change).map_err(FromWaOS::Refused)?;
    }
    let global = context.store.global_settings().map_err(refused)?;
    let own = context.store.program_settings(id).map_err(refused)?;
    Ok(FromWaOS::Settings(rows(&program_page(
        &global, &own, &offered,
    ))))
}

/// Whether a run of `cargo waos run` runs the program `id` now, or ran it once.
async fn was_run_by_cargo_waos(context: &SettingsContext, id: &ProgramId) -> bool {
    context.runs.dev_run(id).await.is_some() || context.components.was_sent(id)
}

fn refused(error: waos_store::StoreError) -> FromWaOS {
    FromWaOS::Refused(format!("{error}"))
}

/// Makes `change` to the program `id`'s own settings, as its page makes it.
fn set_program(store: &Store, id: &ProgramId, change: SettingChange) -> Result<(), String> {
    let global = store
        .global_settings()
        .map_err(|error| format!("{error}"))?;
    let mut refusal = None;
    store
        .change_program_settings(id, |own| {
            refusal = change_program(own, &global, change).err()
        })
        .map_err(|error| format!("{error}"))?;
    refusal.map_or(Ok(()), Err)
}

/// The page's change `row` makes; why not for a machine or an output not offered now.
fn change_of(row: Row, offered: &setting_changes::Offered) -> Result<SettingChange, String> {
    Ok(match row {
        Row::Question(question, Some(answer)) => {
            SettingChange::Answer(store_question(question), Some(store_answer(answer)))
        }
        Row::Question(question, None) => {
            SettingChange::FollowGlobal(Which::Answer(store_question(question)))
        }
        Row::Switch(switch, Some(on)) => switched(switch, on),
        Row::Switch(switch, None) => SettingChange::FollowGlobal(which_switch(switch)),
        Row::LinkScheme(scheme, answer) => {
            SettingChange::LinkScheme(scheme, answer.map(store_answer))
        }
        Row::Role(role, answer) => SettingChange::Role(store_role(role), answer.map(store_answer)),
        Row::Machine(id) => {
            let ids = offered.machines.iter().map(|machine| machine.id.as_str());
            SettingChange::MachineAnswer(one_offered(id, ids, "machine")?)
        }
        Row::AudioOutput(id) => {
            let ids = offered.outputs.iter().map(|output| output.id.as_str());
            SettingChange::AudioOutputAnswer(one_offered(id, ids, "sound output")?)
        }
        Row::SimulatedMicrophone(Some(sound)) => {
            SettingChange::SimulatedMicrophone(store_sound(sound))
        }
        Row::SimulatedMicrophone(None) => SettingChange::FollowGlobal(Which::SimulatedMicrophone),
        Row::FileDialog(dialog, paths) => SettingChange::FileDialogAnswer(
            store_dialog(dialog),
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        ),
    })
}

/// `id`, when it is one of `offered`, or none to ask; why not otherwise.
fn one_offered<'a>(
    id: Option<String>,
    mut offered: impl Iterator<Item = &'a str>,
    what: &str,
) -> Result<Option<String>, String> {
    match id {
        Some(id) if !offered.any(|each| each == id) => Err(format!(
            "no {what} {id:?} is offered now: `cargo waos settings` lists those that are"
        )),
        id => Ok(id),
    }
}

/// The page's rows as `cargo waos settings` prints them, in the page's order.
fn rows(page: &ShownSettings) -> Vec<SettingRow> {
    let mut rows = vec![
        switch_row(
            Switch::Network,
            &page.network.value.on,
            page.network.follows_global,
        ),
        switch_row(
            Switch::LocalNetwork,
            &page.local_network.value,
            page.local_network.follows_global,
        ),
        switch_row(
            Switch::ClipboardRead,
            &page.clipboard_read.value,
            page.clipboard_read.follows_global,
        ),
        switch_row(
            Switch::ClipboardWrite,
            &page.clipboard_write.value,
            page.clipboard_write.follows_global,
        ),
    ];
    for question in Question::ALL {
        rows.push(question_row(
            question,
            page.answers.get(store_question(question)),
        ));
    }
    let sound = &page.simulated_microphone;
    rows.push(SettingRow {
        follows_global: sound.follows_global,
        ..row(
            SIMULATED_MICROPHONE,
            vec![sound_key(sound.value).to_owned()],
        )
    });
    for (scheme, answer) in &page.link_schemes {
        let key = format!("{LINK_SCHEME} {scheme}");
        rows.push(row(&key, vec![answer_key(*answer).to_owned()]));
    }
    for (role, answer) in &page.roles {
        let key = protocol_role(*role).key();
        rows.push(row(key, vec![answer_key(*answer).to_owned()]));
    }
    for dialog in FileDialog::ALL {
        let paths = page.answer_with.paths.file_dialog(store_dialog(dialog));
        let values = paths.map_or_else(
            || vec![ASK.to_owned()],
            |paths| {
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect()
            },
        );
        rows.push(row(dialog.key(), values));
    }
    let output = page
        .answer_with
        .audio_output
        .as_ref()
        .map(|output| output.id.clone());
    let outputs = page.offered.outputs.iter().map(|output| Offered {
        id: output.id.clone(),
        name: output.label.clone(),
    });
    rows.push(picker_row(AUDIO_OUTPUT, output, outputs.collect()));
    let machine = page
        .answer_with
        .machine
        .as_ref()
        .map(|machine| machine.id.clone());
    let machines = page.offered.machines.iter().map(|machine| Offered {
        id: machine.id.clone(),
        name: machine.name.clone(),
    });
    rows.push(picker_row(MACHINE, machine, machines.collect()));
    rows
}

fn row(key: &str, values: Vec<String>) -> SettingRow {
    SettingRow {
        key: key.to_owned(),
        values,
        follows_global: false,
        offered: Vec::new(),
    }
}

fn switch_row(switch: Switch, on: &bool, follows_global: bool) -> SettingRow {
    SettingRow {
        follows_global,
        ..row(
            switch.key(),
            vec![if *on { "on" } else { "off" }.to_owned()],
        )
    }
}

fn question_row(question: Question, shown: &Shown<Option<waos_store::Answer>>) -> SettingRow {
    let value = shown.value.map_or(ASK, answer_key);
    SettingRow {
        follows_global: shown.follows_global,
        ..row(question.key(), vec![value.to_owned()])
    }
}

fn picker_row(key: &str, set: Option<String>, offered: Vec<Offered>) -> SettingRow {
    SettingRow {
        offered,
        ..row(key, vec![set.unwrap_or_else(|| ASK.to_owned())])
    }
}

fn switched(switch: Switch, on: bool) -> SettingChange {
    match switch {
        Switch::Network => SettingChange::NetworkOn(on),
        Switch::LocalNetwork => SettingChange::LocalNetwork(on),
        Switch::ClipboardRead => SettingChange::ClipboardRead(on),
        Switch::ClipboardWrite => SettingChange::ClipboardWrite(on),
    }
}

fn which_switch(switch: Switch) -> Which {
    match switch {
        Switch::Network => Which::Network,
        Switch::LocalNetwork => Which::LocalNetwork,
        Switch::ClipboardRead => Which::ClipboardRead,
        Switch::ClipboardWrite => Which::ClipboardWrite,
    }
}

fn store_question(question: Question) -> waos_store::Question {
    match question {
        Question::Notifications => waos_store::Question::Notifications,
        Question::Camera => waos_store::Question::Camera,
        Question::Microphone => waos_store::Question::Microphone,
    }
}

fn store_role(role: Role) -> waos_store::Role {
    match role {
        Role::Inspector => waos_store::Role::Inspector,
    }
}

fn protocol_role(role: waos_store::Role) -> Role {
    match role {
        waos_store::Role::Inspector => Role::Inspector,
    }
}

fn store_answer(answer: Answer) -> waos_store::Answer {
    match answer {
        Answer::Allow => waos_store::Answer::Granted,
        Answer::Deny => waos_store::Answer::Denied,
        Answer::Simulate => waos_store::Answer::Simulated,
    }
}

fn store_sound(sound: SimulatedSound) -> waos::SimulatedSound {
    match sound {
        SimulatedSound::Tone => waos::SimulatedSound::Tone,
        SimulatedSound::Silence => waos::SimulatedSound::Silence,
    }
}

fn sound_key(sound: waos::SimulatedSound) -> &'static str {
    match sound {
        waos::SimulatedSound::Tone => SimulatedSound::Tone.key(),
        waos::SimulatedSound::Silence => SimulatedSound::Silence.key(),
    }
}

/// `answer` as `cargo waos set` takes it.
fn answer_key(answer: waos_store::Answer) -> &'static str {
    match answer {
        waos_store::Answer::Granted => Answer::Allow.key(),
        waos_store::Answer::Denied => Answer::Deny.key(),
        waos_store::Answer::Simulated => Answer::Simulate.key(),
    }
}

fn store_dialog(dialog: FileDialog) -> waos::FileDialog {
    match dialog {
        FileDialog::OpenFile => waos::FileDialog::OpenFile,
        FileDialog::OpenFiles => waos::FileDialog::OpenFiles,
        FileDialog::OpenFolder => waos::FileDialog::OpenFolder,
        FileDialog::SaveFile => waos::FileDialog::SaveFile,
    }
}
