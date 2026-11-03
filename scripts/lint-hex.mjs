// Fails when a hardcoded hex colour appears under src/widgets/** (SPEC §5.1, DESIGN §2).
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = 'src/widgets';
const HEX = /#[0-9a-fA-F]{3,8}\b/g;
const EXT = /\.(svelte|ts|js|css|json)$/;

/** @param {string} dir @returns {Generator<string>} */
function* walk(dir) {
	for (const name of readdirSync(dir)) {
		const path = join(dir, name);
		if (statSync(path).isDirectory()) yield* walk(path);
		else if (EXT.test(name)) yield path;
	}
}

let failures = 0;
for (const file of walk(ROOT)) {
	readFileSync(file, 'utf8')
		.split('\n')
		.forEach((line, i) => {
			for (const match of line.matchAll(HEX)) {
				failures++;
				console.error(`${relative('.', file)}:${i + 1}: hardcoded colour ${match[0]}, use var(--k-*)`);
			}
		});
}

if (failures > 0) {
	console.error(`\n${failures} hardcoded colour(s) under ${ROOT}/`);
	process.exit(1);
}
console.log(`lint-hex: ${ROOT}/ clean`);
