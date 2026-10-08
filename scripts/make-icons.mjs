// Rasterises Tock's original icons with no image dependencies.
//   node scripts/make-icons.mjs        → src-tauri/icons/tray.png, app-icon.png
//   npx tauri icon app-icon.png        → regenerates the platform icon set
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
const rrect = (x0, y0, x1, y1, r) => (x, y) => {
  const cx = Math.max(x0 + r, Math.min(x, x1 - r));
  const cy = Math.max(y0 + r, Math.min(y, y1 - r));
  return x >= x0 && x <= x1 && y >= y0 && y <= y1 && (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
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

const hex = (h, a = 1) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16), a];
const mix = (c0, c1, t) => c0.map((v, i) => v + (c1[i] - v) * t);

// A bell cap: a small disc with the clock body (plus a gap) cut away.
const bell = (cx, cy) => (x, y) => circle(cx, cy, 0.14)(x, y) && !circle(0.5, 0.55, 0.44)(x, y);

// Tray: monochrome template glyph, a little alarm clock with two bells.
const ink = [0, 0, 0, 1];
const tray = [
  [ring(0.5, 0.55, 0.29, 0.38), ink],
  [arc(0.5, 0.55, 0, 0.22, 0, Math.PI * 1.5), [0, 0, 0, 0.55]],
  [capsule(0.5, 0.55, 0.5, 0.36, 0.075), ink],
  [capsule(0.5, 0.55, 0.64, 0.55, 0.075), ink],
  [bell(0.24, 0.2), ink],
  [bell(0.76, 0.2), ink],
  [capsule(0.3, 0.88, 0.24, 0.96, 0.08), ink],
  [capsule(0.7, 0.88, 0.76, 0.96, 0.08), ink],
];
writeFileSync("src-tauri/icons/tray.png", png(44, render(44, tray, 6)));

// App icon: putty casing, recessed amber dial, burnt-orange accents.
const putty = (x, y) => mix(hex("#f3e8cf"), hex("#cdb68c"), y);
const bezel = (x, y) => mix(hex("#5a3a24"), hex("#1d120b"), y);
const face = (x, y) => {
  const d = Math.hypot(x - 0.5, y - 0.53) / 0.3;
  return mix(hex("#3a2716"), hex("#140d08"), Math.min(1, d));
};
const amber = hex("#ffb347");
const ticks = [];
for (let i = 0; i < 12; i++) {
  const a = (i / 12) * Math.PI * 2;
  const r0 = i % 3 === 0 ? 0.235 : 0.255, r1 = 0.28;
  ticks.push([
    capsule(0.5 + Math.sin(a) * r0, 0.53 - Math.cos(a) * r0, 0.5 + Math.sin(a) * r1, 0.53 - Math.cos(a) * r1, i % 3 === 0 ? 0.022 : 0.014),
    i % 3 === 0 ? amber : hex("#ffb347", 0.55),
  ]);
}
const app = [
  [rrect(0.2, 0.86, 0.32, 0.93, 0.03), hex("#2a1a10")],
  [rrect(0.68, 0.86, 0.8, 0.93, 0.03), hex("#2a1a10")],
  [rrect(0.08, 0.1, 0.92, 0.9, 0.2), hex("#7a5232")],
  [rrect(0.08, 0.09, 0.92, 0.88, 0.2), putty],
  [(x, y) => y < 0.2 && rrect(0.08, 0.09, 0.92, 0.88, 0.2)(x, y), hex("#d8692a")],
  [circle(0.2, 0.145, 0.022), hex("#fff2c4")],
  [circle(0.27, 0.145, 0.022), hex("#ffb347")],
  [circle(0.5, 0.53, 0.34), bezel],
  [ring(0.5, 0.53, 0.3, 0.315), hex("#c99a5b")],
  [circle(0.5, 0.53, 0.3), face],
  [arc(0.5, 0.53, 0.205, 0.225, 0, Math.PI * 1.45), hex("#ff8a2a")],
  ...ticks,
  [capsule(0.5, 0.53, 0.5, 0.34, 0.03), hex("#fff2c4")],
  [capsule(0.5, 0.53, 0.63, 0.6, 0.03), hex("#fff2c4")],
  [circle(0.5, 0.53, 0.03), hex("#d8692a")],
];
writeFileSync("app-icon.png", png(1024, render(1024, app, 3)));
console.log("wrote src-tauri/icons/tray.png and app-icon.png");
