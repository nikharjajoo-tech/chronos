// Rasterises the menu-bar icon with no image dependencies. The app icon itself is
// drawn in app-icon.svg.
//   node scripts/make-icons.mjs        → src-tauri/icons/tray.png
//   npx tauri icon app-icon.svg        → regenerates the platform icon set
import { writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const CRC = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
};
function png(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// Shapes in a unit square (0..1). Each returns true if the point is inside.
const circle = (cx, cy, r) => (x, y) => (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
const ring = (cx, cy, r0, r1) => (x, y) => {
  const d = (x - cx) ** 2 + (y - cy) ** 2;
  return d >= r0 * r0 && d <= r1 * r1;
};
const arc = (cx, cy, r0, r1, a0, a1) => (x, y) => {
  if (!ring(cx, cy, r0, r1)(x, y)) return false;
  let a = Math.atan2(x - cx, cy - y); // 0 at 12 o'clock, clockwise
  if (a < 0) a += Math.PI * 2;
  return a >= a0 && a <= a1;
};
const capsule = (ax, ay, bx, by, w) => (x, y) => {
  const dx = bx - ax, dy = by - ay;
  const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)));
  return (x - ax - t * dx) ** 2 + (y - ay - t * dy) ** 2 <= (w / 2) ** 2;
};
function render(size, layers, ss = 4) {
  const out = Buffer.alloc(size * size * 4);
  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const x = (px + (sx + 0.5) / ss) / size;
          const y = (py + (sy + 0.5) / ss) / size;
          let cr = 0, cg = 0, cb = 0, ca = 0; // premultiplied "over" compositing
          for (const [test, color] of layers) {
            if (!test(x, y)) continue;
            const [lr, lg, lb, la = 1] = typeof color === "function" ? color(x, y) : color;
            cr = lr * la + cr * (1 - la);
            cg = lg * la + cg * (1 - la);
            cb = lb * la + cb * (1 - la);
            ca = la + ca * (1 - la);
          }
          r += cr; g += cg; b += cb; a += ca;
        }
      }
      const n = ss * ss, i = (py * size + px) * 4;
      out[i + 3] = Math.round((a / n) * 255);
      const k = a > 0 ? 1 / a : 0;
      out[i] = Math.round(r * k);
      out[i + 1] = Math.round(g * k);
      out[i + 2] = Math.round(b * k);
    }
  }
  return out;
}

// Tray: monochrome template glyph matching the app icon: a dial with the focus arc
// running from 12 o'clock to about 8, a faint track for the rest, and one hand.
const ink = [0, 0, 0, 1];
const at = (r, a) => [0.5 + Math.sin(a) * r, 0.5 - Math.cos(a) * r]; // 0 at 12 o'clock, clockwise
const end = (235 / 180) * Math.PI;
const tray = [
  [ring(0.5, 0.5, 0.31, 0.44), [0, 0, 0, 0.3]],
  [arc(0.5, 0.5, 0.31, 0.44, 0, end), ink],
  [circle(...at(0.375, 0), 0.065), ink],
  [circle(...at(0.375, end), 0.065), ink],
  [capsule(0.5, 0.5, ...at(0.23, end), 0.08), ink],
  [circle(0.5, 0.5, 0.058), ink],
];
writeFileSync("src-tauri/icons/tray.png", png(44, render(44, tray, 6)));
console.log("wrote src-tauri/icons/tray.png");
