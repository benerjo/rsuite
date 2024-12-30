mod activate;
mod metronome;
mod recorder;
mod transposer;

pub use activate::activator;
use egui_plot::{Line, PlotPoints};
pub use metronome::metronome;
pub use recorder::record;
pub use transposer::transposer;

use crate::synth::{hardware::KeyBoardKey, wavetype::WaveType};

#[derive(Debug)]
pub enum ConnectionType {
    MidiIn,
    MidiOut,
    AudioIn,
    AudioOut,
}

#[derive(Debug)]
pub enum CommonError {
    UnableToStartClient(jack::Error),
    UnableToActivateTheClient(jack::Error),
    UnableToDeActivateClient(jack::Error),
    UnableToStartUserInterface(eframe::Error),
    ConnectionError(ConnectionType, jack::Error),
}

impl std::fmt::Display for CommonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommonError::UnableToStartClient(e) => writeln!(f, "Unable to start the client: {e}"),
            CommonError::UnableToDeActivateClient(e) => {
                writeln!(f, "Unable to de-activate the client: {e}")
            }
            CommonError::UnableToStartUserInterface(e) => {
                writeln!(f, "Unable to start the user interface: {e}")
            }
            CommonError::ConnectionError(c, e) => {
                writeln!(f, "Error while performing connection to {:?}: {e}", c)
            }
            CommonError::UnableToActivateTheClient(e) => {
                writeln!(f, "Unable to activate the client: {e}")
            }
        }
    }
}

impl std::error::Error for CommonError {}

///Start an executable that is located in the same directory
/// as the current one and is named 'command_name'. Note that
/// the '.exe' suffix of windows executables is not needed.
///If anything goes wrong, details will be added to the vector
/// 'messages'
fn start_command(command_name: &str, messages: &mut Vec<String>) {
    let rsynth_arg = match std::env::args().next() {
        Some(v) => v,
        None => {
            messages.push(String::from(
                "Unable to find the name of the current executable",
            ));
            return;
        }
    };

    let rsynt_path = std::path::Path::new(&rsynth_arg);
    if !rsynt_path.exists() {
        messages.push(format!("{rsynth_arg} does not exist!"));
        return;
    }
    let rsynth_dir = match rsynt_path.parent() {
        Some(v) => v,
        None => {
            messages.push(format!("{rsynth_arg} has no parent!"));
            return;
        }
    };
    let mut exe = std::path::PathBuf::from(rsynth_dir);
    if cfg!(target_os = "windows") {
        exe.push(format!("{command_name}.exe"));
    } else {
        exe.push(command_name);
    }
    if !exe.exists() {
        messages.push(format!("Unable to find the executable: {}", exe.display()));
        return;
    }
    if !exe.is_file() {
        messages.push(format!("'{}' is not a file!", exe.display()));
        return;
    }
    if let Err(e) = std::process::Command::new(exe.as_path()).spawn() {
        messages.push(format!("Unable to start process: {e}"));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyBoardKeySetter {
    Set(KeyBoardKey),
    Clear(KeyBoardKey),
}

pub fn create_keyboard_select<T>(
    ui: &mut eframe::egui::Ui,
    name: &str,
    keyboard_key: KeyBoardKey,
    sender: &mut std::sync::mpsc::Sender<T>,
    messages: &mut Vec<String>,
) where
    T: From<KeyBoardKeySetter>,
{
    ui.menu_button(name, |ui| {
        if ui.button("Define...").clicked() {
            if let Err(e) = sender.send(T::from(KeyBoardKeySetter::Set(keyboard_key))) {
                messages.push(format!("Unable to send message to set {keyboard_key}: {e}"));
            } else {
                ui.close_menu();
            }
        }
        if ui.button("Clear").clicked() {
            if let Err(e) = sender.send(T::from(KeyBoardKeySetter::Clear(keyboard_key))) {
                messages.push(format!(
                    "Unable to send message to clear {keyboard_key}: {e}"
                ));
            } else {
                ui.close_menu();
            }
        }
    });
}

///Generate the common menu to lunch the different executables of this crate
pub fn common_menu_luncher(ui: &mut eframe::egui::Ui, messages: &mut Vec<String>) {
    ui.menu_button("Synths", |ui| {
        if ui.button("Kick").clicked() {
            start_command("kick", messages);
            ui.close_menu();
        }
        if ui.button("RSynth").clicked() {
            start_command("rsynth", messages);
            ui.close_menu();
        }
        if ui.button("Snare").clicked() {
            start_command("snare", messages);
            ui.close_menu();
        }
    });
    ui.menu_button("Effects", |ui| {
        if ui.button("Smooth").clicked() {
            start_command("smooth", messages);
            ui.close_menu();
        }
    });
    ui.menu_button("Utils", |ui| {
        if ui.button("Activator").clicked() {
            start_command("activator", messages);
            ui.close_menu();
        }
        if ui.button("Metronome").clicked() {
            start_command("metronome", messages);
            ui.close_menu();
        }
        if ui.button("Recorder").clicked() {
            start_command("recorder", messages);
            ui.close_menu();
        }
        if ui.button("Transposer").clicked() {
            start_command("transposer", messages);
            ui.close_menu();
        }
    });
}

///Generate a line that can be shown in a plot
pub fn create_plot_line(wave: &WaveType) -> Line {
    let mut points = Vec::with_capacity(314 * 2);
    for i in 0..(314 * 2) {
        let x = (i as f64) * (1.0 / 100.0);
        points.push([x, wave.compute(x)]);
    }
    let points = PlotPoints::new(points);
    Line::new(points)
}

///Create a slider from 0 to 128 with a DragValue to show the value
pub fn create_u8_slider(ui: &mut eframe::egui::Ui, label: &str, value: &mut u8) {
    ui.horizontal(|ui| {
        ui.label(label);
        let range = std::ops::RangeInclusive::new(0 as u8, 128 as u8);
        ui.add_enabled(
            true,
            eframe::egui::Slider::new(value, range.clone()).show_value(false),
        );
        ui.add_enabled(
            true,
            eframe::egui::DragValue::new(value)
                .range(range.clone())
                .speed(1),
        );
    });
}

///Create a slider from 0 to 128 with a DragValue to show the value
pub fn create_usize_slider(
    ui: &mut eframe::egui::Ui,
    label: &str,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_enabled(
            true,
            eframe::egui::Slider::new(value, range.clone()).show_value(false),
        );
        let step = ((*range.end() as f64) - (*range.start() as f64)) / 128.0;
        ui.add_enabled(
            true,
            eframe::egui::DragValue::new(value)
                .range(range.clone())
                .speed(step),
        );
    });
}

pub fn create_f64_slider(
    ui: &mut eframe::egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add_enabled(
            true,
            eframe::egui::Slider::new(value, range.clone()).show_value(false),
        );
        ui.add_enabled(
            true,
            eframe::egui::DragValue::new(value)
                .range(range.clone())
                .speed((range.end() - range.start()) / 100.0),
        );
    });
}

///Generate the common UI to show messages
pub fn show_logs(ui: &mut eframe::egui::Ui, messages: &mut Vec<String>) {
    if ui.button("Clear Messages").clicked() {
        messages.clear();
    }
    let text_style = eframe::egui::TextStyle::Body;
    let row_height = ui.text_style_height(&text_style);
    eframe::egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .show_rows(ui, row_height, messages.len(), |ui, row_range| {
            for row in row_range {
                ui.label(&messages[row]);
            }
        });
}
