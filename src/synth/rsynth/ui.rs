use eframe::egui::{self};
use egui_plot::{Line, PlotPoints};
use std::{
    ops::RangeInclusive,
    sync::mpsc::{Receiver, Sender},
};

use crate::{
    nsm,
    synth::{
        hardware::KeyBoardKey,
        rsynth::{
            configuration::Configuration,
            player::{
                MessageToPlayer, MessageToUI, Player, FADE_DURATION_STEP, GAIN_STEP, OVERTONE_STEP,
            },
        },
    },
};

pub struct RustySynth<'c> {
    receiver: Receiver<MessageToUI>,
    commands: Sender<MessageToPlayer>,
    configuration: Configuration,
    messages: Vec<String>,
    used_keys: Vec<KeyBoardKey>,
    //the jack client to make sure that we update the name of the window
    client: &'c jack::Client,
}

impl<'c> RustySynth<'c> {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rcv: Receiver<MessageToUI>,
        send: Sender<MessageToPlayer>,
        client: &'c jack::Client,
        configuration: Configuration,
    ) -> Self {
        nsm::set_ui_context(&cc.egui_ctx);
        return Self {
            receiver: rcv,
            commands: send,
            configuration,
            messages: Vec::new(),
            used_keys: vec![
                KeyBoardKey::WaveSelection,
                KeyBoardKey::Overtone(0),
                KeyBoardKey::Overtone(1),
                KeyBoardKey::Overtone(2),
                KeyBoardKey::Overtone(3),
                KeyBoardKey::Overtone(4),
                KeyBoardKey::Overtone(5),
                KeyBoardKey::Overtone(6),
                KeyBoardKey::Overtone(7),
                KeyBoardKey::Overtone(8),
                KeyBoardKey::FadeInDuration,
                KeyBoardKey::FadeInShape,
                KeyBoardKey::FadeOutDuration,
                KeyBoardKey::FadeOutShape,
                KeyBoardKey::Gain,
                KeyBoardKey::Modulation,
                KeyBoardKey::ModulationSpeed,
                KeyBoardKey::ModulationIntensity,
            ],
            client: client,
        };
    }

    fn create_fade_shape(fade_in: bool, shape: u8, duration: f64) -> Line<'static> {
        let factor = Player::get_shape_factor(shape);
        let mut points = Vec::with_capacity(314 * 2);
        for i in 0..(300) {
            let x = (i as f64) / 300.0;
            let v = if fade_in {
                x.powf(factor)
            } else {
                (1.0 - x).powf(factor)
            };
            points.push([x * duration, v]);
        }
        let points = PlotPoints::new(points);
        Line::new(if fade_in { "fade in" } else { "fade out" }, points)
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            //when a session manager is used, it is in charge of saving the configuration
            if !nsm::is_session_managed() {
                ui.menu_button("File", |ui| {
                    if ui.button("Save Configuration").clicked() {
                        todo!()
                    }
                    if ui.button("Load Configuration").clicked() {
                        todo!()
                    }
                });
            }
            ui.menu_button("Settings", |ui| {
                ui.menu_button("KeyBoard", |ui| {
                    for k in &self.used_keys {
                        crate::utils::create_keyboard_select(
                            ui,
                            &format!("{k}"),
                            *k,
                            &mut self.commands,
                            &mut self.messages,
                        );
                    }
                });
                if ui.button("Clear keyboard mapping").clicked() {
                    match self.commands.send(MessageToPlayer::ClearAllKeyboardKeys) {
                        Ok(()) => {
                            ui.close();
                        }
                        Err(e) => self.messages.push(format!("[UI] {e}")),
                    }
                }
                if ui.button("Save keyboard mapping").clicked() {
                    match self.commands.send(MessageToPlayer::SaveConf) {
                        Ok(()) => {
                            ui.close();
                        }
                        Err(e) => self.messages.push(format!("[UI] {e}")),
                    }
                }
                if ui.button("Load keyboard mapping").clicked() {
                    match self.commands.send(MessageToPlayer::LoadConf) {
                        Ok(()) => {
                            ui.close();
                        }
                        Err(e) => self.messages.push(format!("[UI] {e}")),
                    }
                }
            });
            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_f64_slider(ui: &mut egui::Ui, label: &str, value: &mut f64, step: f64) {
        let range = std::ops::RangeInclusive::new(0.0, 128.0 * step);
        crate::utils::create_f64_slider(ui, label, value, range);
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        let current_config = self.configuration.clone();

        //
        // Wave Type
        //
        ui.horizontal(|ui| {
            ui.label("Wave type:");
            if ui.button(format!("{}", self.configuration.wave)).clicked() {
                self.configuration.wave = self.configuration.wave.cycle();
            }
        });
        let line = crate::utils::create_plot_line(&self.configuration.wave);
        egui_plot::Plot::new(format!("Wave type: {}", &self.configuration.wave))
            .view_aspect(21.0 / 9.0)
            .show(ui, |plot_ui| plot_ui.line(line));

        //
        // Overtones
        //
        ui.label("Overtones:");
        ui.horizontal(|ui| {
            for overtone_index in 0..self.configuration.overtone.len() {
                self.configuration.overtone[overtone_index] =
                    self.configuration.overtone[overtone_index];
                let range = std::ops::RangeInclusive::new(0.0, 128.0 * OVERTONE_STEP);
                ui.add_enabled(
                    true,
                    egui::Slider::new(&mut self.configuration.overtone[overtone_index], range)
                        .show_value(false)
                        .text(format!(
                            "{:.2}",
                            self.configuration.overtone_freq[overtone_index]
                        ))
                        .vertical(),
                );
            }
        });

        //
        // Fade in
        //
        ui.label("Fade in: ");
        Self::create_f64_slider(
            ui,
            "duration: ",
            &mut self.configuration.fade_in_duration,
            FADE_DURATION_STEP,
        );

        crate::utils::create_u8_slider(ui, "shape: ", &mut self.configuration.fade_in_shape);
        egui_plot::Plot::new(format!("Fade in:"))
            .view_aspect(42.0 / 9.0)
            .show(ui, |plot_ui| {
                plot_ui.line(Self::create_fade_shape(
                    true,
                    self.configuration.fade_in_shape,
                    self.configuration.fade_in_duration,
                ))
            });

        //
        // Fade out
        //
        ui.label("Fade out: ");

        Self::create_f64_slider(
            ui,
            "duration: ",
            &mut self.configuration.fade_out_duration,
            FADE_DURATION_STEP,
        );
        crate::utils::create_u8_slider(ui, "shape: ", &mut self.configuration.fade_out_shape);
        egui_plot::Plot::new(format!("Fade out:"))
            .view_aspect(42.0 / 9.0)
            .show(ui, |plot_ui| {
                plot_ui.line(Self::create_fade_shape(
                    false,
                    self.configuration.fade_out_shape,
                    self.configuration.fade_out_duration,
                ))
            });

        //
        // Gain
        //
        Self::create_f64_slider(ui, "Gain: ", &mut self.configuration.gain, GAIN_STEP);

        crate::utils::create_u8_slider(ui, "Modulation: ", &mut self.configuration.modulation);
        crate::utils::create_f64_slider(
            ui,
            "Modulation speed",
            &mut self.configuration.mod_speed,
            RangeInclusive::new(0.0, 32.0),
        );
        crate::utils::create_f64_slider(
            ui,
            "Modulation intensity",
            &mut self.configuration.mod_intensity,
            RangeInclusive::new(0.0, 1.0),
        );

        if current_config != self.configuration {
            if let Err(e) = self.commands.send(MessageToPlayer::NewConfiguration(
                self.configuration.clone(),
            )) {
                self.messages
                    .push(format!("Unable to send configuration to player: {e}"));
            }
        }

        crate::utils::show_logs(ui, &mut self.messages);
    }
}

impl<'c> eframe::App for RustySynth<'c> {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //the following line should only occur if a repaint is really needed.
        //To do this, we need to revise the architecture to make sure that it
        //can be called whenever there is someting in the queue
        ctx.request_repaint();

        let new_title = String::from(self.client.name());
        ctx.send_viewport_cmd(egui::viewport::ViewportCommand::Title(new_title));

        while let Ok(m) = self.receiver.try_recv() {
            match m {
                MessageToUI::Error(e) => self.messages.push(format!("Error: {e}")),
                MessageToUI::NewConfiguration(configuration) => self.configuration = configuration,
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical(|ui| {
                self.create_menu(ui);
                self.create_content(ui);
            });
        });
    }
}
