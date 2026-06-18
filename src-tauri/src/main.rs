#![cfg_attr(windows, windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod modules;

pub(crate) use modules::*;

fn main() {
    bootstrap::run();
}
