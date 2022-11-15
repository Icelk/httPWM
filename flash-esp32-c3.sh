#!/bin/sh

echo If this errors, consider running chmod 777 /dev/ttyUSB0

espflash --partition-table esp32-c3-partitions.csv /dev/ttyUSB0 target/riscv32imc-esp-espidf/distribution/httpwmd
