use eframe::egui::{self};
use egui_plot::{Line, PlotPoints};
use std::sync::mpsc::{Receiver, Sender};

use crate::synth::{
    hardware::KeyBoardKey,
    rsynth::{
        configuration::{Configuration, ConfigurationChange},
        player::{
            ExternalPlayerInput, Player, PlayerChange, FADE_DURATION_STEP, GAIN_STEP, OVERTONE_STEP,
        },
    },
    wavetype::WaveType,
};

pub struct RustySynth<'c> {
    receiver: Receiver<PlayerChange>,
    config_changes: Receiver<ConfigurationChange>,
    commands: Sender<ExternalPlayerInput>,
    configuration: Configuration,
    wave_type: WaveType,
    overtones_impact: Vec<f64>,
    overtones_freq: Vec<f64>,
    messages: Vec<String>,
    fade_in_duration: f64,
    shape_fade_in_selector: u8,
    shape_fade_in: f64,
    fade_out_duration: f64,
    shape_fade_out: f64,
    shape_fade_out_selector: u8,
    gain: f64,
    used_keys: Vec<KeyBoardKey>,
    //the jack client to make sure that we update the name of the window
    client: &'c jack::Client,
}

impl<'c> RustySynth<'c> {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        rcv: Receiver<PlayerChange>,
        config_rcv: Receiver<ConfigurationChange>,
        send: Sender<ExternalPlayerInput>,
        config_snd: Sender<ConfigurationChange>,
        client: &'c jack::Client,
    ) -> Self {
        let mut new = Self {
            receiver: rcv,
            config_changes: config_rcv,
            commands: send,
            configuration: Configuration::new(),
            wave_type: WaveType::default(),
            overtones_impact: Configuration::default_overtone_impact(),
            overtones_freq: Configuration::default_overtone_frequencies(),
            messages: Vec::new(),
            fade_in_duration: 0.1,
            shape_fade_in: 1.0,
            shape_fade_in_selector: 64,
            fade_out_duration: 0.1,
            shape_fade_out: 1.0,
            shape_fade_out_selector: 64,
            gain: 1.0,
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
            ],
            client: client,
        };
        new.configuration.set_change_listener(config_snd);
        return new;
    }

    fn create_fade_shape(fade_in: bool, factor: f64, duration: f64) -> Line {
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
        Line::new(points)
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Save Configuration").clicked() {
                    todo!()
                }
                if ui.button("Load Configuration").clicked() {
                    todo!()
                }
            });
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
                    match self
                        .commands
                        .send(ExternalPlayerInput::ClearAllKeyboardKeys)
                    {
                        Ok(()) => {
                            ui.close_menu();
                        }
                        Err(e) => self.messages.push(format!("[UI] {e}")),
                    }
                }
                if ui.button("Save keyboard mapping").clicked() {
                    match self.commands.send(ExternalPlayerInput::SaveConf) {
                        Ok(()) => {
                            ui.close_menu();
                        }
                        Err(e) => self.messages.push(format!("[UI] {e}")),
                    }
                }
                if ui.button("Load keyboard mapping").clicked() {
                    match self.commands.send(ExternalPlayerInput::LoadConf) {
                        Ok(()) => {
                            ui.close_menu();
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
        //
        // Wave Type
        //
        ui.horizontal(|ui| {
            ui.label("Wave type:");
            if ui
                .button(format!("{}", self.configuration.wave()))
                .clicked()
            {
                self.configuration.cycle_wave_type()
            }
        });
        let line = crate::utils::create_plot_line(&self.configuration.wave());
        egui_plot::Plot::new(format!("Wave type: {}", self.configuration.wave()))
            .view_aspect(21.0 / 9.0)
            .show(ui, |plot_ui| plot_ui.line(line));

        //
        // Overtones
        //
        ui.label("Overtones:");
        ui.horizontal(|ui| {
            for overtone_index in 0..self.overtones_impact.len() {
                self.configuration
                    .update_overtone(overtone_index, self.overtones_impact[overtone_index]);
                let range = std::ops::RangeInclusive::new(0.0, 128.0 * OVERTONE_STEP);
                ui.add_enabled(
                    true,
                    egui::Slider::new(&mut self.overtones_impact[overtone_index], range)
                        .show_value(false)
                        .text(format!("{:.2}", self.overtones_freq[overtone_index]))
                        .vertical(),
                );
            }
        });

        //
        // Fade in
        //
        ui.label("Fade in: ");
        self.configuration
            .set_fade_in_duration(self.fade_in_duration);
        Self::create_f64_slider(
            ui,
            "duration: ",
            &mut self.fade_in_duration,
            FADE_DURATION_STEP,
        );
        crate::utils::create_u8_slider(ui, "shape: ", &mut self.shape_fade_in_selector);
        self.shape_fade_in = Player::get_shape_factor(self.shape_fade_in_selector);
        self.configuration.set_fade_in_shape(self.shape_fade_in);
        egui_plot::Plot::new(format!("Fade in:"))
            .view_aspect(42.0 / 9.0)
            .show(ui, |plot_ui| {
                plot_ui.line(Self::create_fade_shape(
                    true,
                    self.shape_fade_in,
                    self.fade_in_duration,
                ))
            });

        //
        // Fade out
        //
        ui.label("Fade out: ");
        self.configuration
            .set_fade_out_duration(self.fade_out_duration);

        Self::create_f64_slider(
            ui,
            "duration: ",
            &mut self.fade_out_duration,
            FADE_DURATION_STEP,
        );
        crate::utils::create_u8_slider(ui, "shape: ", &mut self.shape_fade_out_selector);
        self.shape_fade_out = Player::get_shape_factor(self.shape_fade_out_selector);
        self.configuration.set_fade_in_shape(self.shape_fade_out);
        egui_plot::Plot::new(format!("Fade out:"))
            .view_aspect(42.0 / 9.0)
            .show(ui, |plot_ui| {
                plot_ui.line(Self::create_fade_shape(
                    false,
                    self.shape_fade_out,
                    self.fade_out_duration,
                ))
            });

        //
        // Gain
        //
        self.configuration.set_gain(self.gain);
        Self::create_f64_slider(ui, "Gain: ", &mut self.gain, GAIN_STEP);

        crate::utils::show_logs(ui, &mut self.messages);
    }
}

impl<'c> eframe::App for RustySynth<'c> {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //the following line should only occur if a repaint is really needed.
        //To do this, we need to revise the architecture to make sure that it
        //can be called whenever there is someting in the queue
        ctx.request_repaint();

        let new_title = String::from(self.client.name());
        ctx.send_viewport_cmd(egui::viewport::ViewportCommand::Title(new_title));

        while let Ok(m) = self.receiver.try_recv() {
            match m {
                PlayerChange::Message(msg) => self.messages.push(msg),
                PlayerChange::Error(e) => self.messages.push(format!("Error: {e}")),
            }
        }

        while let Ok(m) = self.config_changes.try_recv() {
            self.configuration.apply_and_send_notifications(m.clone());
            match m {
                ConfigurationChange::Wave(w) => {
                    self.wave_type = w;
                }
                ConfigurationChange::Overtone { index, value } => {
                    while self.overtones_impact.len() <= index {
                        self.overtones_impact.push(0.0);
                    }
                    self.overtones_impact[index] = value;
                }
                ConfigurationChange::FadeInDuration(v) => self.fade_in_duration = v,
                ConfigurationChange::FadeOutDuration(v) => self.fade_out_duration = v,
                ConfigurationChange::ShapeFactorFadeIn(v) => self.shape_fade_in = v,
                ConfigurationChange::ShapeFactorFadeOut(v) => self.shape_fade_out = v,
                ConfigurationChange::Gain(g) => self.gain = g,
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                self.create_menu(ui);
                self.create_content(ui);
            });
        });
    }
}
