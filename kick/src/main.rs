#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod kick;

use kick::kick;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = kick() {
        println!("Error: {e}");
    }
}
