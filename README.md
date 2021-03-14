# *htt***PWM**

> httPWM is a web application driven by a custom backend.

The backend consists of my web server [Iselk/kvarn](https://github.com/Iselk/kvarn)
and a custom-built event-loop.

It's goal is to enable software control over PWM output on the Raspberry Pi,
either through the hardware `PWM channels` or through `software PWM`.

The backend event-loop and scheduler handling is the core part, and can be used without `Kvarn` and other binary dependencies.
If you want to use only the library, check out [main.rs](src/bin/main.rs) for a implementation and make sure to disable default features in your `Cargo.toml`.


# State of project

Right now, it's in a working state and can be deployed on a Raspberry Pi. Major improvements and UI changes are imminent.

It takes about 5 minutes to compile the first time on a `RPi model 3`.


# Contribution

This code is licensed under the MIT license, and so should all contributions also be.

If you encounter any bugs, please open an issue or tackle the problem yourself!
