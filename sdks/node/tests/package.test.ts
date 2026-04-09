import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { describe, expect, it } from 'vitest';

const __dirname = dirname(fileURLToPath(import.meta.url));
const packageJsonPath = resolve(__dirname, '..', 'package.json');
const packageJson = JSON.parse(readFileSync(packageJsonPath, 'utf8')) as {
  main: string;
  module: string;
  exports: {
    '.': {
      import: string;
      require: string;
      types: string;
    };
  };
};

describe('published package manifest', () => {
  it('references build outputs that exist', () => {
    const entrypoints = [
      packageJson.main,
      packageJson.module,
      packageJson.exports['.'].require,
      packageJson.exports['.'].import,
      packageJson.exports['.'].types,
    ];

    for (const entrypoint of entrypoints) {
      expect(existsSync(resolve(__dirname, '..', entrypoint))).toBe(true);
    }
  });

  it('loads the CommonJS entrypoint advertised by package.json', async () => {
    const mod = await import(resolve(__dirname, '..', packageJson.main));
    expect(mod.Agent).toBeTypeOf('function');
  });

  it('loads the ESM entrypoint advertised by package.json', async () => {
    const mod = await import(pathToFileURL(resolve(__dirname, '..', packageJson.module)).href);
    expect(mod.Agent).toBeTypeOf('function');
  });
});
