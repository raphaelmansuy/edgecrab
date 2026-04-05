#!/usr/bin/env node
/**
 * Postinstall: download the correct EdgeCrab native binary for the current
 * platform and architecture from GitHub Releases.
 *
 * Supported platforms:
 *   darwin-arm64   → edgecrab-aarch64-apple-darwin.tar.gz
 *   darwin-x64     → edgecrab-x86_64-apple-darwin.tar.gz
 *   linux-x64      → edgecrab-x86_64-unknown-linux-gnu.tar.gz
 *   linux-arm64    → edgecrab-aarch64-unknown-linux-gnu.tar.gz
 *   win32-x64      → edgecrab-x86_64-pc-windows-msvc.zip
 */

'use strict';

const https = require('node:https');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { execSync } = require('node:child_process');
const { createGunzip } = require('node:zlib');

// ─── Config ──────────────────────────────────────────────────────────────────
const VERSION = require('../package.json').version;
const REPO    = 'raphaelmansuy/edgecrab';
const BINARY  = process.platform === 'win32' ? 'edgecrab.exe' : 'edgecrab';
const BIN_DIR = path.join(__dirname, '..', 'bin');
const DEST    = path.join(BIN_DIR, BINARY);

// ─── Platform map ─────────────────────────────────────────────────────────────
const PLATFORM_MAP = {
  'darwin-arm64':  `edgecrab-aarch64-apple-darwin.tar.gz`,
  'darwin-x64':    `edgecrab-x86_64-apple-darwin.tar.gz`,
  'linux-x64':     `edgecrab-x86_64-unknown-linux-gnu.tar.gz`,
  'linux-arm64':   `edgecrab-aarch64-unknown-linux-gnu.tar.gz`,
  'win32-x64':     `edgecrab-x86_64-pc-windows-msvc.zip`,
};

const key = `${process.platform}-${os.arch()}`;
const archive = PLATFORM_MAP[key];

if (!archive) {
  console.error(`[edgecrab-cli] Unsupported platform: ${key}`);
  console.error(`[edgecrab-cli] Please build from source: https://github.com/${REPO}`);
  process.exit(0); // non-fatal — SDK still works
}

const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${archive}`;

// Skip download in CI if binary already exists (cache hit)
if (fs.existsSync(DEST)) {
  console.log(`[edgecrab-cli] Binary already present: ${DEST}`);
  process.exit(0);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/**
 * Follow redirects and return the final response.
 * @param {string} downloadUrl
 * @returns {Promise<import('node:http').IncomingMessage>}
 */
function fetchFollowRedirects(downloadUrl) {
  return new Promise((resolve, reject) => {
    const req = https.get(downloadUrl, { headers: { 'User-Agent': 'edgecrab-npm-installer' } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        resolve(fetchFollowRedirects(res.headers.location));
      } else if (res.statusCode === 200) {
        resolve(res);
      } else {
        reject(new Error(`HTTP ${res.statusCode} for ${downloadUrl}`));
      }
    });
    req.on('error', reject);
  });
}

/**
 * Extract the binary from a .tar.gz archive into BIN_DIR.
 * @param {string} tarPath  path to the downloaded .tar.gz
 */
function extractTarGz(tarPath) {
  // Use system tar if available (macOS and Linux always have it)
  execSync(`tar -xzf "${tarPath}" -C "${BIN_DIR}" --strip-components=0 "${BINARY}" 2>/dev/null || tar -xzf "${tarPath}" -C "${BIN_DIR}"`, { stdio: 'pipe' });
}

/**
 * Extract the binary from a .zip archive into BIN_DIR.
 * @param {string} zipPath
 */
function extractZip(zipPath) {
  // PowerShell is available on all modern Windows
  execSync(`powershell -Command "Expand-Archive -Path '${zipPath}' -DestinationPath '${BIN_DIR}' -Force"`, { stdio: 'pipe' });
}

// ─── Main ─────────────────────────────────────────────────────────────────────
async function main() {
  fs.mkdirSync(BIN_DIR, { recursive: true });

  const tmpFile = path.join(os.tmpdir(), `edgecrab-install-${Date.now()}.${archive.endsWith('.zip') ? 'zip' : 'tar.gz'}`);

  console.log(`[edgecrab-cli] Downloading ${archive} ...`);

  try {
    const res = await fetchFollowRedirects(url);
    await new Promise((resolve, reject) => {
      const out = fs.createWriteStream(tmpFile);
      res.pipe(out);
      out.on('finish', resolve);
      out.on('error', reject);
    });
  } catch (err) {
    console.error(`[edgecrab-cli] Download failed: ${err.message}`);
    console.error(`[edgecrab-cli] You can install manually: cargo install edgecrab-cli`);
    process.exit(0); // non-fatal
  }

  try {
    if (archive.endsWith('.zip')) {
      extractZip(tmpFile);
    } else {
      extractTarGz(tmpFile);
    }
    fs.chmodSync(DEST, 0o755);
    console.log(`[edgecrab-cli] Installed: ${DEST}`);
  } catch (err) {
    console.error(`[edgecrab-cli] Extraction failed: ${err.message}`);
    console.error(`[edgecrab-cli] You can install manually: cargo install edgecrab-cli`);
  } finally {
    try { fs.unlinkSync(tmpFile); } catch (_) { /* ignore */ }
  }
}

main().catch((err) => {
  console.error(`[edgecrab-cli] Unexpected error: ${err.message}`);
  process.exit(0); // always non-fatal to avoid blocking npm install
});
