#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod distortion;

use distortion::distortion;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = distortion() {
        println!("Error: {e}");
    }
}
