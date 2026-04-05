#!/bin/sh
cd /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab
/Users/raphaelmansuy/.rustup/toolchains/1.88.0-aarch64-apple-darwin/bin/cargo check 2>&1
echo "RC=$?"
