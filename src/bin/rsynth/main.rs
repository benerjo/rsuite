#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, ViewportBuilder};
use rsuite::synth::rsynth::ui::RustySynth;
use std::sync::mpsc::channel;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    // open client
    let (client, _status) = match jack::Client::new("RSynth", jack::ClientOptions::NO_START_SERVER)
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error while trying to set up client: {e}");
            std::process::exit(4)
        }
    };

    let client_name = String::from(client.name());

    //create a sync channel to send back copies of midi messages we get
    let (player_change_sender, player_change_receiver) = channel();
    let (config_change_sender, config_change_receiver) = channel();
    let (ui_config_change_sender, ui_config_change_receiver) = channel();
    //create a sync channel to send non midi commands to the player
    let (external_command_send, external_command_receive) = channel();
    let mut synth =
        rsuite::synth::rsynth::player::Player::new(&client, external_command_receive).unwrap();
    synth.set_change_listener(
        player_change_sender,
        config_change_sender,
        ui_config_change_receiver,
    );
    let active_client = match client.activate_async((), synth) {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Error while trying to activate the client: {e}");
            std::process::exit(1)
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
                config_change_receiver,
                external_command_send,
                ui_config_change_sender,
                &active_client.as_client(),
            )))
        }),
    ) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error while trying to start the user interface: {e}");
            std::process::exit(3)
        }
    }

    match active_client.deactivate() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error while trying to de-activate the client: {e}");
            std::process::exit(2)
        }
    }
}
