use crate::nsm;
use crate::utils::CommonError;
use crate::{nsm::Session, synth::rsynth::ui::RustySynth};
use eframe::egui::{self, ViewportBuilder};
use std::sync::mpsc::channel;

mod configuration;
mod player;
mod ui;

pub fn rsynth() -> Result<(), CommonError> {
    // open client
    let session = Session::start("RSynth")?;
    let client = session.client("RSynth")?;

    let client_name = session.window_title(client.name());

    //create a sync channel to send back copies of midi messages we get
    let (player_change_sender, player_change_receiver) = channel();
    //create a sync channel to send non midi commands to the player
    let (external_command_send, external_command_receive) = channel();
    let state = session.load(configuration::SynthState {
        config: configuration::Configuration::new(),
        keyboard: player::Player::default_keyboard(),
    });
    let config = state.config.clone();
    let synth = player::Player::new(
        &client,
        external_command_receive,
        player_change_sender,
        state.clone(),
    )
    .unwrap();
    let active_client = session.activate(client, synth, state)?;

    nsm::run_window(
        &client_name,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| {
            Ok(Box::new(RustySynth::new(
                cc,
                player_change_receiver,
                external_command_send,
                &active_client.as_client(),
                config,
            )))
        }),
    )?;

    match active_client.deactivate() {
        Ok(_) => {}
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
    Ok(())
}
