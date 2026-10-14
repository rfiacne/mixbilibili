//! Process adapter: run the library, print what it says failed, exit.

use mixbilibili::i18n::t;
use mixbilibili::ui::{Colors, Symbols};

fn main() {
    if let Err(failure) = mixbilibili::run() {
        let symbols = Symbols::new(mixbilibili::ui::supports_unicode());
        let prefix = Colors::failure(&format!("{} {}", symbols.failure, t("error_prefix")));
        eprintln!("{} {}", prefix, failure);
        std::process::exit(failure.exit_code);
    }
}
