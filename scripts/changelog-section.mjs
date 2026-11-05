#!/usr/bin/env node
// Prints one section of CHANGELOG.md (Keep a Changelog format) to stdout.
//
//   node scripts/changelog-section.mjs 1.0.0        # "## [1.0.0] - 2026-11-20"
//   node scripts/changelog-section.mjs v1.0.0       # a leading "v" is ignored
//   node scripts/changelog-section.mjs Unreleased
//
// The heading line itself is not printed. Exits 1 if the section is missing or empty,
// so a release cannot go out with an empty body.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const arg = process.argv[2];
if (!arg) {
	console.error('usage: changelog-section.mjs <version|Unreleased> [CHANGELOG.md]');
	process.exit(2);
}
const version = arg.replace(/^v(?=\d)/, '');
const file =
	process.argv[3] ?? join(dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md');
const lines = readFileSync(file, 'utf8').split(/\r?\n/);

const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const heading = new RegExp(`^##\\s+\\[?${escaped}\\]?(\\s|$)`, 'i');
const start = lines.findIndex((l) => heading.test(l));
if (start === -1) {
	console.error(`changelog-section: no "## [${version}]" heading in ${file}`);
	process.exit(1);
}

const body = [];
for (const line of lines.slice(start + 1)) {
	if (/^##\s/.test(line)) break; // next release
	if (/^\[[^\]]+\]:\s*\S+/.test(line)) break; // link reference definitions at the end
	body.push(line);
}
const text = body.join('\n').trim();
if (!text) {
	console.error(`changelog-section: section "${version}" is empty`);
	process.exit(1);
}
process.stdout.write(text + '\n');
