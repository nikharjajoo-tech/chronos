import type { ReactElement } from "react";

// Amber 7-segment readout with faint "ghost" segments behind the lit ones,
// like an old instrument panel. Pure SVG, so it scales cleanly at any size.

const SEGMENTS: Record<string, string> = {
  "0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc",
  "5": "afgcd", "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg",
  "-": "g", " ": "",
};

const W = 60, H = 100, T = 11, G = 2.2;
const L = 7, R = W - 7, TOP = 7, MID = H / 2, BOT = H - 7;

function hSeg(y: number) {
  const a = L + G, b = R - G, h = T / 2;
  return `${a},${y} ${a + h},${y - h} ${b - h},${y - h} ${b},${y} ${b - h},${y + h} ${a + h},${y + h}`;
}
function vSeg(x: number, y0: number, y1: number) {
  const a = y0 + G, b = y1 - G, h = T / 2;
  return `${x},${a} ${x + h},${a + h} ${x + h},${b - h} ${x},${b} ${x - h},${b - h} ${x - h},${a + h}`;
}

const GEOMETRY: Record<string, string> = {
  a: hSeg(TOP), g: hSeg(MID), d: hSeg(BOT),
  f: vSeg(L, TOP, MID), b: vSeg(R, TOP, MID),
  e: vSeg(L, MID, BOT), c: vSeg(R, MID, BOT),
};

function Digit({ ch, x }: { ch: string; x: number }) {
  const lit = SEGMENTS[ch] ?? "";
  return (
    <g transform={`translate(${x} 0) skewX(-6)`}>
      {Object.entries(GEOMETRY).map(([k, pts]) => (
        <polygon key={k} points={pts} className={lit.includes(k) ? "seg on" : "seg"} />
      ))}
    </g>
  );
}

const COLON_W = 26, GAP = 10;

export function SevenSeg({ text, blinkColon = false }: { text: string; blinkColon?: boolean }) {
  let x = 4;
  const parts: ReactElement[] = [];
  [...text].forEach((ch, i) => {
    if (ch === ":") {
      parts.push(
        <g key={i} className={blinkColon ? "colon blink" : "colon"} transform={`translate(${x} 0)`}>
          <rect x={COLON_W / 2 - 5} y={30} width={9} height={9} rx={1.5} className="seg on" />
          <rect x={COLON_W / 2 - 8} y={62} width={9} height={9} rx={1.5} className="seg on" />
        </g>,
      );
      x += COLON_W;
    } else {
      parts.push(<Digit key={i} ch={ch} x={x} />);
      x += W + GAP;
    }
  });
  return (
    <svg className="sevenseg" viewBox={`0 0 ${x - GAP + 4} ${H}`} aria-label={text} role="img">
      {parts}
    </svg>
  );
}
