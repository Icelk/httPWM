#!/usr/bin/sh
rsync --del . -rhP pi@pi:/home/pi/pwm_dev/ --exclude target
