#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod a440;

use a440::a440;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = a440() {
        println!("Error: {e}");
    }
}
