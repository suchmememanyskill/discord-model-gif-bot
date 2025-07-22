#!/bin/sh

cd /app
echo "Hello world!"
Xvfb&
xvfb-run ./discord-model-gif-bot