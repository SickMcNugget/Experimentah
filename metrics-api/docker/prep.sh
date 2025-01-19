#!/bin/sh

echo "Prepping..."
if [ ! -d "/data/prometheus" ]; then
    mkdir -p "/data/prometheus"
fi

chown -R "65534:65534" "/data/prometheus"
echo "Prepped"
