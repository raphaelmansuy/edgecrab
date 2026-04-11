import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const dataDir = dirname(fileURLToPath(import.meta.url));
const repoRootCandidates = [
  process.env.EDGECRAB_REPO_ROOT,
  join(process.cwd(), '..'),
  join(dataDir, '..', '..', '..'),
].filter((value): value is string => Boolean(value));

const repoRoot =
  repoRootCandidates.find(
    (candidate) =>
      existsSync(join(candidate, 'Cargo.toml')) &&
      existsSync(join(candidate, 'crates')) &&
      existsSync(join(candidate, 'site'))
  ) ?? repoRootCandidates[0];

function readRepoFile(path: string): string {
  return readFileSync(join(repoRoot, path), 'utf8');
}

function readWorkspaceVersion(): string {
  const cargoToml = readRepoFile('Cargo.toml');
  const match = cargoToml.match(/\[workspace\.package\][\s\S]*?version = "([^"]+)"/);
  if (!match) {
    throw new Error('failed to read workspace version from Cargo.toml');
  }
  return match[1];
}

function countQuotedItemsInArray(source: string, constName: string): number {
  const start = source.indexOf(`pub const ${constName}`);
  if (start === -1) {
    throw new Error(`missing ${constName} in source`);
  }
  const end = source.indexOf('];', start);
  return source
    .slice(start, end)
    .split('\n')
    .filter((line) => line.trim().startsWith('"')).length;
}

function countTopLevelProviderEntries(): number {
  const yaml = readRepoFile('crates/edgecrab-core/src/model_catalog_default.yaml');
  const lines = yaml.split('\n');
  let inProviders = false;
  let count = 0;

  for (const line of lines) {
    if (line.startsWith('providers:')) {
      inProviders = true;
      continue;
    }
    if (!inProviders) {
      continue;
    }
    if (line && !line.startsWith(' ')) {
      break;
    }
    if (
      line.startsWith('  ') &&
      !line.startsWith('    ') &&
      line.trim().endsWith(':') &&
      !line.trim().startsWith('#')
    ) {
      count += 1;
    }
  }

  return count;
}

function countGatewayPlatforms(): number {
  const source = readRepoFile('crates/edgecrab-cli/src/gateway_catalog.rs');
  const start = source.indexOf('const PLATFORMS: &[GatewayPlatformDef] = &[');
  if (start === -1) {
    throw new Error('missing gateway platform catalog');
  }
  const end = source.indexOf('];', start);
  return source
    .slice(start, end)
    .split('\n')
    .filter((line) => line.includes('id:'))
    .length;
}

const toolsetsSource = readRepoFile('crates/edgecrab-tools/src/toolsets.rs');

export const workspaceVersion = readWorkspaceVersion();
export const coreToolCount = countQuotedItemsInArray(toolsetsSource, 'CORE_TOOLS');
export const providerCount = countTopLevelProviderEntries();
export const gatewayCount = countGatewayPlatforms();

export const releaseFacts = {
  workspaceVersion,
  coreToolCount,
  providerCount,
  gatewayCount,
} as const;
