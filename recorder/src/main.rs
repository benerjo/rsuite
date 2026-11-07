#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod recorder;
mod wavwriter;

use recorder::record;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = record() {
        println!("Error: {e}");
    }
}
