mod cli;
mod error;
mod ffmpeg;
mod i18n;
#[path = "merger/mod.rs"]
mod merger;
mod progress;
mod scanner;
mod state;
mod ui;
mod workflow;

use crate::error::{get_exit_code, translate_error};
use crate::i18n::t;
use crate::ui::{Colors, Symbols};
use std::sync::atomic::AtomicBool;

/// Shared flag set by the Ctrl+C handler.
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn main() {
    if let Err(e) = workflow::run() {
        let symbols = Symbols::new(ui::supports_unicode());
        let error_prefix = Colors::failure(&format!("{} {}", symbols.failure, t("error_prefix")));
        eprintln!("{} {}", error_prefix, translate_error(&e));
        std::process::exit(get_exit_code(&e));
    }
}
