#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod rsynth;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = rsynth::rsynth() {
        eprintln!("Error: {e}");
    }
}
