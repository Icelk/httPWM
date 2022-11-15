#!/bin/sh

ESP_IDF_VERSION="release/v4.4" ESP_IDF_SDKCONFIG_DEFAULTS="sdkconfig.defaults" RUSTFLAGS="-Clinker=ldproxy -Cdefault-linker-libraries" cargo +nightly build --target riscv32imc-esp-espidf --profile distribution --no-default-features -F bin,esp32 -Z unstable-options -Z build-std=std,panic_abort

# ESP_IDF_VERSION="release/v4.4" ESP_IDF_SDKCONFIG_DEFAULTS="sdkconfig.defaults" RUSTFLAGS="-Clinker=ldproxy -Cdefault-linker-libraries" cargo +nightly espflash --target riscv32imc-esp-espidf --profile distribution --no-default-features --features bin,esp32 -Z unstable-options -Z build-std=std,panic_abort
