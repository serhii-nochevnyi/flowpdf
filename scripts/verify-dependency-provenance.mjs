import assert from 'node:assert/strict';
import { readFile, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

export function normalizeRepository(value) {
  if (typeof value !== 'string' || value.length === 0) return null;
  const normalized = value
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/\.git(?:#.*)?$/, '')
    .replace(/\/$/, '');
  return /^https:\/\/(?:www\.)?github\.com\/[\w.-]+\/[\w.-]+$/i.test(normalized)
    ? normalized.replace(/^https:\/\/www\./, 'https://')
    : null;
}

export async function verifyManifest() {
  throw new Error('RED: provenance verifier has not been implemented');
}

test('accepts an allowlisted stable crate and npm release with complete provenance', async () => {
  await assert.doesNotReject(() => verifyManifest({}));
});

test('normalizes git+https repository URLs', () => {
  assert.equal(
    normalizeRepository('git+https://github.com/example/repository.git'),
    'https://github.com/example/repository',
  );
});

if (process.argv.includes('--config')) {
  const input = process.argv.slice(2);
  const valueAfter = (flag) => input[input.indexOf(flag) + 1];
  const configPath = valueAfter('--config');
  const reportPath = valueAfter('--report');
  const blockerPath = valueAfter('--blocker');
  if (!configPath || !reportPath || !blockerPath) {
    throw new Error('Usage: --config <path> --report <path> --blocker <path>');
  }
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  await verifyManifest({ config, reportPath, blockerPath });
}
