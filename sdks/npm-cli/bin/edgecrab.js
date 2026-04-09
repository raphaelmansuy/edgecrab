#!/usr/bin/env node
/**
 * Thin launcher for the EdgeCrab native binary.
 *
 * This script is the `bin` entry point for `edgecrab-cli` on npm.
 * It resolves the platform-specific binary installed by `postinstall`,
 * then spawns it with all arguments forwarded.
 */

'use strict';

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const BINARY  = process.platform === 'win32' ? 'edgecrab.exe' : 'edgecrab';
const BIN_DIR = path.join(__dirname, BINARY);
const PACKAGE_VERSION = require('../package.json').version;

if (!fs.existsSync(BIN_DIR)) {
  console.error(
    `[edgecrab-cli] Native binary not found at: ${BIN_DIR}\n` +
    `[edgecrab-cli] Re-run: npm install edgecrab-cli\n` +
    `[edgecrab-cli] Or install from source: cargo install edgecrab-cli`
  );
  process.exit(1);
}

const result = spawnSync(BIN_DIR, process.argv.slice(2), {
  stdio: 'inherit',
  env: {
    ...process.env,
    EDGECRAB_INSTALL_METHOD: 'npm',
    EDGECRAB_WRAPPER_VERSION: PACKAGE_VERSION,
    EDGECRAB_BINARY_VERSION: PACKAGE_VERSION,
  },
});

process.exit(result.status ?? 1);
