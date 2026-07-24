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
const { execSync, spawnSync } = require('node:child_process');

// ─── Config ──────────────────────────────────────────────────────────────────
// First principle: the wrapper package version is the binary release version.
// Release automation must change package.json only; every runtime consumer
// derives the binary tag from that single value.
const BINARY_VERSION = require('../package.json').version;
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

const url = `https://github.com/${REPO}/releases/download/v${BINARY_VERSION}/${archive}`;
const VERSION_RE = /\bedgecrab\s+([0-9]+\.[0-9]+\.[0-9]+)\b/i;

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

function readBinaryVersion(binaryPath) {
  if (!fs.existsSync(binaryPath)) {
    return null;
  }
  try {
    const result = spawnSync(binaryPath, ['--version'], {
      encoding: 'utf8',
      timeout: 5000,
      env: {
        ...process.env,
        EDGECRAB_INSTALL_METHOD: 'npm',
        EDGECRAB_WRAPPER_VERSION: BINARY_VERSION,
        EDGECRAB_BINARY_VERSION: BINARY_VERSION,
        EDGECRAB_NO_PATH_REPAIR: '1',
      },
    });
    const output = `${result.stdout || ''}\n${result.stderr || ''}`;
    const match = output.match(VERSION_RE);
    return match ? match[1] : null;
  } catch (_) {
    return null;
  }
}

function hasCurrentBinary(binaryPath) {
  return readBinaryVersion(binaryPath) === BINARY_VERSION;
}

function pathRepairDisabled() {
  const v = String(process.env.EDGECRAB_NO_PATH_REPAIR || '').trim();
  return v === '1' || /^true$/i.test(v) || /^yes$/i.test(v);
}

function parseSemverTriplet(ver) {
  const core = String(ver || '').split('-')[0];
  const parts = core.split('.');
  if (parts.length < 3) return null;
  const major = Number(parts[0]);
  const minor = Number(parts[1]);
  const patch = Number(parts[2]);
  if (![major, minor, patch].every((n) => Number.isInteger(n) && n >= 0)) {
    return null;
  }
  return [major, minor, patch];
}

function versionIsStrictlyOlder(other, running) {
  const a = parseSemverTriplet(other);
  const b = parseSemverTriplet(running);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i += 1) {
    if (a[i] < b[i]) return true;
    if (a[i] > b[i]) return false;
  }
  return false;
}

function samePath(a, b) {
  try {
    return fs.realpathSync(a) === fs.realpathSync(b);
  } catch (_) {
    return path.resolve(a) === path.resolve(b);
  }
}

/** True for ELF / Mach-O / PE native binaries (not Node/shell/Python wrappers). */
function isNativeBinary(filePath) {
  try {
    const resolved = fs.realpathSync(filePath);
    const fd = fs.openSync(resolved, 'r');
    const buf = Buffer.alloc(4);
    const n = fs.readSync(fd, buf, 0, 4, 0);
    fs.closeSync(fd);
    if (n < 2) return false;
    if (buf[0] === 0x7f && buf[1] === 0x45 && buf[2] === 0x4c && buf[3] === 0x46) {
      return true; // ELF
    }
    if (buf[0] === 0xcf && buf[1] === 0xfa && buf[2] === 0xed && buf[3] === 0xfe) {
      return true; // Mach-O 64 LE
    }
    if (buf[0] === 0xce && buf[1] === 0xfa && buf[2] === 0xed && buf[3] === 0xfe) {
      return true; // Mach-O 32 LE
    }
    if (buf[0] === 0xca && buf[1] === 0xfe && buf[2] === 0xba && buf[3] === 0xbe) {
      return true; // Mach-O fat
    }
    if (buf[0] === 0x4d && buf[1] === 0x5a) {
      return true; // PE
    }
    return false;
  } catch (_) {
    return false;
  }
}

function bakPathFor(candidate, version) {
  const parent = path.dirname(candidate);
  const safeVer = String(version).replace(/[^0-9A-Za-z.-]/g, '_');
  let dest = path.join(parent, `edgecrab.${safeVer}.bak`);
  if (!fs.existsSync(dest)) return dest;
  for (let n = 2; n < 100; n += 1) {
    dest = path.join(parent, `edgecrab.${safeVer}.bak.${n}`);
    if (!fs.existsSync(dest)) return dest;
  }
  return path.join(parent, `edgecrab.${safeVer}.bak.${Date.now()}`);
}

/**
 * Rename strictly older native `edgecrab` binaries on PATH to `.bak`.
 * Mirrors the Rust launch-time repair. Opt out: EDGECRAB_NO_PATH_REPAIR=1.
 *
 * @param {string} [runningVersion]
 * @param {string|null} [selfBinary] absolute path of the package-managed binary to skip
 * @returns {Array<[string, string]>}
 */
function repairStalePathBinaries(runningVersion = BINARY_VERSION, selfBinary = DEST) {
  if (pathRepairDisabled()) {
    return [];
  }

  const pathEnv = process.env.PATH || process.env.Path || '';
  const dirs = pathEnv.split(path.delimiter).filter(Boolean);
  const renamed = [];
  const seen = [];

  for (const dir of dirs) {
    const candidate = path.join(dir, BINARY);
    if (!fs.existsSync(candidate)) continue;
    try {
      if (!fs.statSync(candidate).isFile()) continue;
    } catch (_) {
      continue;
    }
    if (!isNativeBinary(candidate)) continue;
    if (seen.some((p) => samePath(p, candidate))) continue;
    if (selfBinary && samePath(selfBinary, candidate)) {
      seen.push(candidate);
      continue;
    }
    seen.push(candidate);

    const otherVer = readBinaryVersion(candidate);
    if (!otherVer || !versionIsStrictlyOlder(otherVer, runningVersion)) {
      continue;
    }

    const dest = bakPathFor(candidate, otherVer);
    try {
      fs.renameSync(candidate, dest);
      renamed.push([candidate, dest]);
    } catch (err) {
      console.error(
        `[edgecrab-cli] PATH repair: could not rename ${candidate}: ${err.message}`
      );
    }
  }

  if (renamed.length > 0) {
    const summary = renamed.map(([from, to]) => `${from} → ${to}`).join('; ');
    console.error(
      `[edgecrab-cli] PATH repair: renamed older edgecrab install(s) so ${runningVersion} is not shadowed — ${summary}. Opt out: EDGECRAB_NO_PATH_REPAIR=1`
    );
  }

  return renamed;
}

function logReadyHint() {
  console.log(
    `[edgecrab-cli] Ready: edgecrab ${BINARY_VERSION}. Launch/postinstall auto-renames older PATH installs (e.g. ~/.cargo/bin/edgecrab) to *.bak. Opt out: EDGECRAB_NO_PATH_REPAIR=1. Check: which -a edgecrab`
  );
}

// ─── Main ─────────────────────────────────────────────────────────────────────
async function ensureInstalledBinary() {
  fs.mkdirSync(BIN_DIR, { recursive: true });

  if (hasCurrentBinary(DEST)) {
    console.log(`[edgecrab-cli] Binary already present: ${DEST}`);
    logReadyHint();
    repairStalePathBinaries();
    return DEST;
  }

  if (fs.existsSync(DEST)) {
    const existingVersion = readBinaryVersion(DEST) || 'unknown';
    console.log(
      `[edgecrab-cli] Replacing stale binary at ${DEST} (found ${existingVersion}, need ${BINARY_VERSION})`
    );
    fs.rmSync(DEST, { force: true });
  }

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
    const installedVersion = readBinaryVersion(DEST);
    if (installedVersion !== BINARY_VERSION) {
      throw new Error(
        `Installed binary version mismatch: expected ${BINARY_VERSION}, got ${installedVersion || 'unknown'}`
      );
    }
    console.log(`[edgecrab-cli] Installed: ${DEST}`);
    logReadyHint();
    repairStalePathBinaries();
    return DEST;
  } catch (err) {
    console.error(`[edgecrab-cli] Extraction failed: ${err.message}`);
    console.error(`[edgecrab-cli] You can install manually: cargo install edgecrab-cli`);
    return null;
  } finally {
    try { fs.unlinkSync(tmpFile); } catch (_) { /* ignore */ }
  }
}

async function main() {
  await ensureInstalledBinary();
}

if (require.main === module) {
  main().catch((err) => {
    console.error(`[edgecrab-cli] Unexpected error: ${err.message}`);
    process.exit(0); // always non-fatal to avoid blocking npm install
  });
}

module.exports = {
  BINARY_VERSION,
  DEST,
  ensureInstalledBinary,
  readBinaryVersion,
  repairStalePathBinaries,
};
