#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rsuite::synth::snare;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = snare() {
        println!("Error: {e}");
    }
}
