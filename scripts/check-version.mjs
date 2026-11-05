#!/usr/bin/env node
// Checks that package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json carry the
// same version, and (optionally) that it matches a release tag.
//
//   node scripts/check-version.mjs            # the three files agree
//   node scripts/check-version.mjs v1.0.0     # ...and equal the tag
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (/** @type {string} */ p) => readFileSync(join(root, p), 'utf8');

const cargoPackage = read('src-tauri/Cargo.toml').split(/^\[/m).find((s) => s.startsWith('package]'));
const versions = {
	'package.json': JSON.parse(read('package.json')).version,
	'src-tauri/Cargo.toml': cargoPackage?.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
	'src-tauri/tauri.conf.json': JSON.parse(read('src-tauri/tauri.conf.json')).version
};

const tag = process.argv[2];
const expected = tag ? tag.replace(/^refs\/tags\//, '').replace(/^v/, '') : versions['package.json'];

let ok = true;
for (const [file, v] of Object.entries(versions)) {
	const match = v === expected;
	ok &&= match;
	console.log(`${match ? 'ok  ' : 'FAIL'} ${file}: ${v ?? '(missing)'}`);
}
if (!ok) {
	console.error(`Version mismatch: expected ${expected}${tag ? ` (from tag ${tag})` : ''}.`);
	process.exit(1);
}
