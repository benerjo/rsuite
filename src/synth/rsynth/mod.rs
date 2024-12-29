use crate::synth::rsynth::ui::RustySynth;
use crate::utils::CommonError;
use eframe::egui::{self, ViewportBuilder};
use std::sync::mpsc::channel;

mod configuration;
mod player;
mod ui;

pub fn rsynth() -> Result<(), CommonError> {
    // open client
    let (client, _status) = match jack::Client::new("RSynth", jack::ClientOptions::NO_START_SERVER)
    {
        Ok(v) => v,
        Err(e) => {
            return Err(CommonError::UnableToStartClient(e));
        }
    };

    let client_name = String::from(client.name());

    //create a sync channel to send back copies of midi messages we get
    let (player_change_sender, player_change_receiver) = channel();
    //create a sync channel to send non midi commands to the player
    let (external_command_send, external_command_receive) = channel();
    let synth =
        player::Player::new(&client, external_command_receive, player_change_sender).unwrap();
    let active_client = match client.activate_async((), synth) {
        Ok(client) => client,
        Err(e) => {
            return Err(CommonError::UnableToStartClient(e));
        }
    };

    match eframe::run_native(
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
            )))
        }),
    ) {
        Ok(_) => {}
        Err(e) => {
            return Err(CommonError::UnableToStartUserInterface(e));
        }
    }

    match active_client.deactivate() {
        Ok(_) => {}
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
    Ok(())
}
