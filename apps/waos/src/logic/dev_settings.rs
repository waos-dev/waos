//! A program's settings as `waos settings` reads them and `waos settings set` sets them: its
//! settings page's rows (`setting_changes`), each by the key `waos-dev-protocol` names it with, a
//! set made as the page makes the same change, so the command and the page are one rule. A set
//! answer is the program's setting, used by every run of it.
//!
//! The invariant: every program's settings are read and set, however it was installed, a published
//! one as a development install, and an id not installed, which may be set before its install: the
//! dev socket is its user's alone.

use std::sync::Arc;

use waos_dev_protocol::{
    ASK, AUDIO_OUTPUT, Answer, FileDialog, LINK_SCHEME, MACHINE, Offered, Question, Refusal,
    RefusalKind, Role, Row, SIMULATED_MICROPHONE, ServerConnection, SettingRow, SettingsAnswer,
    SimulatedSound, Switch,
};
use waos_package::ProgramId;
use waos_permissions::Permissions;

use super::dev_refusal;
use super::dev_server::BUILD;
use super::environments::ProgramEnvironments;
use super::setting_changes::{
    self, SettingChange, Shown, ShownSettings, Which, change_program, program_page,
};

/// What reading and setting a program's settings needs of the app.
pub(crate) struct SettingsContext {
    pub(crate) permissions: Permissions,
    pub(crate) environments: Arc<ProgramEnvironments>,
}

/// Answers a connection's `Settings`, with `set` none, or its `Set`: the rows of the program
/// `id`'s settings, after the change where one is, or why it could not answer.
pub(crate) async fn answer(
    mut connection: ServerConnection,
    context: &SettingsContext,
    id: &ProgramId,
    set: Option<Row>,
) {
    let answer = match rows_after(context, id, set).await {
        Ok(rows) => SettingsAnswer::Rows(rows),
        Err(refusal) => SettingsAnswer::Refused(refusal),
    };
    // A client gone meanwhile has nobody to tell.
    let _ = connection.send(&answer).await;
}

/// The rows of the program `id`'s settings after `set`, or why they could not be read or set.
async fn rows_after(
    context: &SettingsContext,
    id: &ProgramId,
    set: Option<Row>,
) -> Result<Vec<SettingRow>, Refusal> {
    let environments = Arc::clone(&context.environments);
    // Listing the sound outputs waits for the system's audio.
    let offered = tokio::task::spawn_blocking(move || environments.offered())
        .await
        .map_err(|error| failed(format!("reading what the pickers offer: {error}")))?;
    if let Some(row) = set {
        let change = change_of(row, &offered)?;
        tracing::info!(%id, ?change, "waos sets a program's setting");
        set_program(&context.permissions, id, change).map_err(failed)?;
    }
    let global = context.permissions.global_settings().map_err(failed)?;
    let own = context.permissions.app_settings(id).map_err(failed)?;
    Ok(rows(&program_page(&global, &own, &offered)))
}

/// A request of the settings' own that failed: its message is the error's.
fn failed(error: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalKind::Failed, error.to_string())
}

/// Makes `change` to the program `id`'s own settings, as its page makes it.
fn set_program(
    permissions: &Permissions,
    id: &ProgramId,
    change: SettingChange,
) -> Result<(), String> {
    let global = permissions
        .global_settings()
        .map_err(|error| format!("{error}"))?;
    let mut refusal = None;
    permissions
        .change_app_settings(id, |own| {
            refusal = change_program(own, &global, change).err()
        })
        .map_err(|error| format!("{error}"))?;
    refusal.map_or(Ok(()), Err)
}

/// The page's change `row` makes; why not for a machine or an output not offered now, or a row
/// from a newer client that this build does not know.
fn change_of(row: Row, offered: &setting_changes::Offered) -> Result<SettingChange, Refusal> {
    let unsupported = || dev_refusal::unsupported(BUILD, "setting");
    Ok(match row {
        Row::Question(question, Some(answer)) => SettingChange::Answer(
            store_question(question).ok_or_else(unsupported)?,
            Some(store_answer(answer).ok_or_else(unsupported)?),
        ),
        Row::Question(question, None) => SettingChange::FollowGlobal(Which::Answer(
            store_question(question).ok_or_else(unsupported)?,
        )),
        Row::Switch(switch, Some(on)) => switched(switch, on).ok_or_else(unsupported)?,
        Row::Switch(switch, None) => {
            SettingChange::FollowGlobal(which_switch(switch).ok_or_else(unsupported)?)
        }
        Row::LinkScheme(scheme, answer) => SettingChange::LinkScheme(
            scheme,
            answer
                .map(|answer| store_answer(answer).ok_or_else(unsupported))
                .transpose()?,
        ),
        Row::Role(role, answer) => SettingChange::Role(
            store_role(role).ok_or_else(unsupported)?,
            answer
                .map(|answer| store_answer(answer).ok_or_else(unsupported))
                .transpose()?,
        ),
        Row::Machine(id) => {
            let ids = offered.machines.iter().map(|machine| machine.id.as_str());
            SettingChange::MachineAnswer(one_offered(id, ids, "machine").map_err(failed)?)
        }
        Row::AudioOutput(id) => {
            let ids = offered.outputs.iter().map(|output| output.id.as_str());
            SettingChange::AudioOutputAnswer(one_offered(id, ids, "sound output").map_err(failed)?)
        }
        Row::SimulatedMicrophone(Some(sound)) => {
            SettingChange::SimulatedMicrophone(store_sound(sound).ok_or_else(unsupported)?)
        }
        Row::SimulatedMicrophone(None) => SettingChange::FollowGlobal(Which::SimulatedMicrophone),
        Row::FileDialog(dialog, paths) => SettingChange::FileDialogAnswer(
            store_dialog(dialog).ok_or_else(unsupported)?,
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        ),
        Row::Unknown => return Err(unsupported()),
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
            "no {what} {id:?} is offered now: `waos settings` lists those that are"
        )),
        id => Ok(id),
    }
}

/// The page's rows as `waos settings` prints them, in the page's order.
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
        switch_row(
            Switch::Record,
            &page.record.value,
            page.record.follows_global,
        ),
    ];
    for question in Question::ALL {
        if let Some(store) = store_question(question) {
            rows.push(question_row(question, page.answers.get(store)));
        }
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
        if let Some(store) = store_dialog(dialog) {
            let paths = page.answer_with.paths.file_dialog(store);
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

fn question_row(question: Question, shown: &Shown<Option<waos_permissions::Answer>>) -> SettingRow {
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

fn switched(switch: Switch, on: bool) -> Option<SettingChange> {
    Some(match switch {
        Switch::Network => SettingChange::NetworkOn(on),
        Switch::LocalNetwork => SettingChange::LocalNetwork(on),
        Switch::ClipboardRead => SettingChange::ClipboardRead(on),
        Switch::ClipboardWrite => SettingChange::ClipboardWrite(on),
        Switch::Record => SettingChange::Record(on),
        Switch::Unknown => return None,
    })
}

fn which_switch(switch: Switch) -> Option<Which> {
    Some(match switch {
        Switch::Network => Which::Network,
        Switch::LocalNetwork => Which::LocalNetwork,
        Switch::ClipboardRead => Which::ClipboardRead,
        Switch::ClipboardWrite => Which::ClipboardWrite,
        Switch::Record => Which::Record,
        Switch::Unknown => return None,
    })
}

fn store_question(question: Question) -> Option<waos_permissions::Question> {
    Some(match question {
        Question::Notifications => waos_permissions::Question::Notifications,
        Question::Camera => waos_permissions::Question::Camera,
        Question::Microphone => waos_permissions::Question::Microphone,
        Question::Unknown => return None,
    })
}

fn store_role(role: Role) -> Option<waos_permissions::Role> {
    Some(match role {
        Role::Inspector => waos_permissions::Role::Inspector,
        Role::Unknown => return None,
    })
}

fn protocol_role(role: waos_permissions::Role) -> Role {
    match role {
        waos_permissions::Role::Inspector => Role::Inspector,
    }
}

fn store_answer(answer: Answer) -> Option<waos_permissions::Answer> {
    Some(match answer {
        Answer::Allow => waos_permissions::Answer::Granted,
        Answer::Deny => waos_permissions::Answer::Denied,
        Answer::Simulate => waos_permissions::Answer::Simulated,
        Answer::Unknown => return None,
    })
}

fn store_sound(sound: SimulatedSound) -> Option<waos::SimulatedSound> {
    Some(match sound {
        SimulatedSound::Tone => waos::SimulatedSound::Tone,
        SimulatedSound::Silence => waos::SimulatedSound::Silence,
        SimulatedSound::Unknown => return None,
    })
}

fn sound_key(sound: waos::SimulatedSound) -> &'static str {
    match sound {
        waos::SimulatedSound::Tone => SimulatedSound::Tone.key(),
        waos::SimulatedSound::Silence => SimulatedSound::Silence.key(),
    }
}

/// `answer` as `waos set` takes it.
fn answer_key(answer: waos_permissions::Answer) -> &'static str {
    match answer {
        waos_permissions::Answer::Granted => Answer::Allow.key(),
        waos_permissions::Answer::Denied => Answer::Deny.key(),
        waos_permissions::Answer::Simulated => Answer::Simulate.key(),
    }
}

fn store_dialog(dialog: FileDialog) -> Option<waos::FileDialog> {
    Some(match dialog {
        FileDialog::OpenFile => waos::FileDialog::OpenFile,
        FileDialog::OpenFiles => waos::FileDialog::OpenFiles,
        FileDialog::OpenFolder => waos::FileDialog::OpenFolder,
        FileDialog::SaveFile => waos::FileDialog::SaveFile,
        FileDialog::Unknown => return None,
    })
}
