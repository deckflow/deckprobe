// Platform tarballs are published before the SDK. Complete their lock entries
// after publication without allowing any other locked dependency to change.
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';

const manifest = JSON.parse(readFileSync('package.json', 'utf8'));
const original = readFileSync('package-lock.json', 'utf8');
const before = JSON.parse(original);
const allowed = new Set(Object.entries(manifest.optionalDependencies ?? {}).map(([name, version]) => {
  assert(name.startsWith('@deckflow/deckprobe-'), `Unexpected optional dependency: ${name}`);
  assert.equal(version, manifest.version, `${name} must match the SDK version`);
  return `node_modules/${name}`;
}));
assert.equal(allowed.size, 7, 'Expected seven native platform packages');
try {
  execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', [
    'install', '--package-lock-only', '--ignore-scripts', '--registry=https://registry.npmjs.org',
  ], { stdio: 'inherit' });
  const after = JSON.parse(readFileSync('package-lock.json', 'utf8'));
  for (const key of allowed) {
    const entry = after.packages[key];
    assert.equal(entry?.version, manifest.version, `${key} is not published`);
    assert.equal(entry.optional, true);
    assert(entry.integrity && entry.resolved?.startsWith('https://registry.npmjs.org/'));
    delete before.packages[key];
    delete after.packages[key];
  }
  assert.deepEqual(after, before, 'Refusing changes outside native platform lock entries');
} catch (error) {
  writeFileSync('package-lock.json', original);
  throw error;
}
console.log('Platform lock entries are complete; all other locked dependencies are unchanged.');
