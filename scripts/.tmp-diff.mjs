import { PNG } from 'pngjs';
import { readFileSync } from 'node:fs';
const [, , a, b] = process.argv;
const A = PNG.sync.read(readFileSync(a)), B = PNG.sync.read(readFileSync(b));
let n = 0, x0 = 1e9, y0 = 1e9, x1 = -1, y1 = -1, maxd = 0;
for (let y = 0; y < A.height; y++) for (let x = 0; x < A.width; x++) {
  const i = (y * A.width + x) * 4;
  const d = Math.max(...[0, 1, 2].map((k) => Math.abs(A.data[i + k] - B.data[i + k])));
  if (d) { n++; maxd = Math.max(maxd, d); x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y); }
}
console.log(b.split(/[\/]/).pop(), `${A.width}x${A.height}`, 'px', n, 'maxΔ', maxd, 'box', x0, y0, x1, y1);
