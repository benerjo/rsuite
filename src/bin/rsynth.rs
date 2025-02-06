#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = rsuite::synth::rsynth::rsynth() {
        eprintln!("Error: {e}");
    }
}
