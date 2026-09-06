// AppKit/sips exports top-down BMPs. NSIS LoadImageCanLoadFile explicitly
// rejects them: preserve the pixels but store rows in bottom-up order.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = JSON.parse(readFileSync(resolve(root, 'tauri.conf.json'), 'utf8'));
const nsis = config.bundle.windows.nsis;
for (const path of [nsis.headerImage, nsis.sidebarImage]) {
  const destination = resolve(root, path);
  const input = readFileSync(destination);
  assert.equal(input.toString('ascii', 0, 2), 'BM');
  assert.equal(input.readUInt32LE(14), 40, 'Expected BITMAPINFOHEADER');
  assert.equal(input.readUInt16LE(26), 1, 'Expected one color plane');
  assert.equal(input.readUInt16LE(28), 24, 'Expected 24-bit RGB');
  assert.equal(input.readUInt32LE(30), 0, 'Expected uncompressed RGB');
  const width = input.readInt32LE(18);
  const rawHeight = input.readInt32LE(22);
  assert.ok(width > 0 && rawHeight !== 0);
  if (rawHeight > 0) continue;
  const height = -rawHeight;
  const offset = input.readUInt32LE(10);
  const stride = Math.ceil(width * 3 / 4) * 4;
  assert.ok(offset >= 54 && offset + stride * height <= input.length, 'Truncated BMP');
  const output = Buffer.from(input);
  for (let row = 0; row < height; row++) {
    input.copy(output, offset + row * stride,
      offset + (height - 1 - row) * stride, offset + (height - row) * stride);
  }
  output.writeInt32LE(height, 22);
  writeFileSync(destination, output);
  console.log(`Normalized NSIS bitmap row order: ${path}`);
}
