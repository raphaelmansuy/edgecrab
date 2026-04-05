#!/usr/bin/env node
/**
 * Post-install script: ensures sharp's prebuilt native binary is available.
 *
 * Baileys depends on sharp for thumbnail generation. sharp v0.34+ ships
 * platform-specific prebuilt binaries as optional npm packages (e.g.
 * @img/sharp-darwin-arm64). When `npm install` is run with `--ignore-scripts`
 * (which we do to avoid the fragile node-gyp build), these prebuilts don't
 * get installed automatically. This script detects the current platform/arch
 * and installs the correct prebuilt package if sharp isn't already functional.
 */

import { execSync } from 'child_process';
import { platform, arch } from 'os';

const PLATFORM_MAP = {
  darwin: 'darwin',
  linux: 'linux',
  win32: 'win32',
  linuxmusl: 'linuxmusl',
};

const ARCH_MAP = {
  arm64: 'arm64',
  x64: 'x64',
  ia32: 'ia32',
  arm: 'arm',
};

function isSharpWorking() {
  try {
    execSync('node -e "require(\'sharp\')"', { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
}

function main() {
  if (isSharpWorking()) {
    console.log('[sharp] Already functional — skipping prebuilt install');
    return;
  }

  const os = PLATFORM_MAP[platform()] || platform();
  const cpu = ARCH_MAP[arch()] || arch();

  // On Alpine/musl Linux, use the musl variant
  let actualOs = os;
  if (os === 'linux') {
    try {
      execSync('ldd --version 2>&1 | grep -i musl', { stdio: 'ignore' });
      actualOs = 'linuxmusl';
    } catch {
      // glibc — keep 'linux'
    }
  }

  const pkg = `@img/sharp-${actualOs}-${cpu}`;
  console.log(`[sharp] Installing prebuilt: ${pkg}`);

  try {
    execSync(`npm install ${pkg} --no-save --ignore-scripts`, {
      stdio: 'inherit',
    });
    console.log(`[sharp] Prebuilt installed successfully`);
  } catch (err) {
    console.warn(`[sharp] Failed to install prebuilt (${pkg}): thumbnails may not work`);
    console.warn('[sharp] This is non-fatal — the bridge will still function');
  }
}

main();
