#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod reverb;

use reverb::reverb;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = reverb() {
        println!("Error: {e}");
    }
}
