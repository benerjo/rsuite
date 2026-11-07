#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod chorus;

use chorus::chorus;

#[cfg(target_os = "windows")]
#[link(name = "C:/Program Files/JACK2/lib/libjack64")]
extern "C" {}

fn main() {
    if let Err(e) = chorus() {
        println!("Error: {e}");
    }
}
