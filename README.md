# _htt_**PWM**

> httPWM is a web-driven PWM controller supported by a custom backend.

The backend consists of my web server [Icelk/kvarn](https://github.com/Icelk/kvarn)
and a custom-built event-loop.

It's goal is to enable software control over PWM output on the Raspberry Pi and ESP32,
either through the hardware `PWM channels` or through `software PWM` (only Raspberry Pi).

The backend event-loop and scheduler handling is the core part, and can be used without `Kvarn` and other binary dependencies.
If you want to use only the library, check out [main.rs](src/bin/main.rs) for a implementation and make sure to disable default features in your `Cargo.toml`.

# Sample circuit

> This is the exact circuit I'm using. You can definitely change the MOSFET to a NPN transistor.

A [sample circuit](resources/circuit.cddx) is included (opened using the [Circuit Diagram Web Editor](https://www.circuit-diagram.org/editor/)),
which uses the Raspberry Pi's hardware PWM pins and 5V power from it's USB ports.

![circuit](resources/circuit.svg)

# State of project

Right now, it's in a working state and can be deployed on a Raspberry Pi and ESP32-C3 (see the build scripts).

It takes about 5 minutes to compile the first time on a `RPi model 3`.
Compilation to the ESP32-C3 is done thought the `./build-esp32.sh` script. It can then be flashed using `./flash-esp32-c3.sh`.
You'll need a few programs installed (`cargo install espmonitor espflash`) and possibly some other dependencies. The script will tell you.

# Contribution

This code is licensed under the MIT license, and so should all contributions also be.

If you encounter any bugs, please open an issue or tackle the problem yourself!
