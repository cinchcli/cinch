#!/usr/bin/env node
// Builds the standalone cinch CLI for the current target and stages
// it at src-tauri/binaries/cinch-<target-triple>(.exe) so Tauri's
// externalBin can pick it up. Used on Windows; macOS uses argv-based
// dispatch into the desktop binary (see apps/desktop/CLAUDE.md
// "CLI Embedding") and this script is a no-op there.

import { execSync } from 'node:child_process';
import { existsSync, mkdirSync, copyFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
// scripts/ → apps/desktop/ → apps/ → repo root
const projectRoot = resolve(here, '..', '..', '..');

function getDefaultTargetTriple() {
  // Matches `rustc -vV | grep host`
  const output = execSync('rustc -vV', { encoding: 'utf-8' });
  const match = output.match(/host:\s*(\S+)/);
  if (!match) {
    throw new Error('Could not determine rustc host target');
  }
  return match[1];
}

const target = process.env.CARGO_TARGET || getDefaultTargetTriple();
const isWindows = target.includes('windows');
const ext = isWindows ? '.exe' : '';

if (!isWindows) {
  console.log(`prepare-cli-binary: skipping (target=${target} is not Windows)`);
  console.log('On macOS the desktop bundle uses argv-dispatch into the embedded CLI.');
  process.exit(0);
}

console.log(`prepare-cli-binary: building cinch CLI for ${target}`);
execSync(
  `cargo build -p cinch-cli --release --target ${target}`,
  { stdio: 'inherit', cwd: projectRoot }
);

const sourceBin = resolve(projectRoot, `target/${target}/release/cinch${ext}`);
if (!existsSync(sourceBin)) {
  throw new Error(`Built binary not found at ${sourceBin}`);
}

const destDir = resolve(projectRoot, 'apps/desktop/src-tauri/binaries');
const destBin = resolve(destDir, `cinch-${target}${ext}`);

mkdirSync(destDir, { recursive: true });
copyFileSync(sourceBin, destBin);
console.log(`prepare-cli-binary: staged ${destBin}`);
