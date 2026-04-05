#!/bin/bash
export PATH="/Users/raphaelmansuy/.cargo/bin:$PATH"
cd /Users/raphaelmansuy/Github/temp/nous_hermes/edgecrab
cargo check 2>&1
echo "EXIT_CODE=$?"
