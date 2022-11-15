#[cfg(feature = "esp32")]
use embuild::{
    self,
    build::{CfgArgs, LinkArgs},
};

#[cfg(not(feature = "esp32"))]
fn main() {}
#[cfg(feature = "esp32")]
fn main() {
    // Necessary because of this issue: https://github.com/rust-lang/cargo/issues/9641
    LinkArgs::output_propagated("ESP_IDF").unwrap();

    let cfg = CfgArgs::try_from_env("ESP_IDF").unwrap();

    cfg.output();
}
