#!/usr/bin/sh
rsync --del . -rhP pi@pi:/home/pi/httPWM/ --filter=':- .gitignore'
