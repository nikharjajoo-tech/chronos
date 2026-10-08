// The recessed dial: bezel, 60 tick marks, and a countdown arc with a brass
// bead riding its tip. Updates step once per second with a short eased
// transition, so it reads as a mechanical tick rather than a smooth sweep.

const C = 100;
const ARC_R = 74;

const TICKS = Array.from({ length: 60 }, (_, i) => {
  const major = i % 5 === 0;
  const a = (i / 60) * Math.PI * 2;
  const r0 = major ? 81 : 84;
  const r1 = 88;
  return {
    major,
    x1: C + Math.sin(a) * r0, y1: C - Math.cos(a) * r0,
    x2: C + Math.sin(a) * r1, y2: C - Math.cos(a) * r1,
  };
});

export function Dial({ fraction }: { fraction: number }) {
  const f = Math.min(1, Math.max(0, fraction));
  return (
    <svg className="dial" viewBox="0 0 200 200" aria-hidden>
      <defs>
        <radialGradient id="face" cx="50%" cy="42%" r="60%">
          <stop offset="0%" stopColor="var(--face-hi)" />
          <stop offset="100%" stopColor="var(--face-lo)" />
        </radialGradient>
        <linearGradient id="bezel" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#6b462b" />
          <stop offset="100%" stopColor="#1a100a" />
        </linearGradient>
        <radialGradient id="glass" cx="35%" cy="22%" r="70%">
          <stop offset="0%" stopColor="rgba(255,240,210,0.16)" />
          <stop offset="45%" stopColor="rgba(255,240,210,0.03)" />
          <stop offset="100%" stopColor="rgba(255,240,210,0)" />
        </radialGradient>
      </defs>
      <circle cx={C} cy={C} r={99} fill="url(#bezel)" />
      <circle cx={C} cy={C} r={92.5} fill="none" stroke="var(--brass)" strokeWidth={1.6} />
      <circle cx={C} cy={C} r={91.5} fill="url(#face)" />
      {TICKS.map(({ major, ...pts }, i) => (
        <line key={i} {...pts} className={major ? "tick major" : "tick"} />
      ))}
      <circle cx={C} cy={C} r={ARC_R} className="arc-track" pathLength={1000} />
      <g transform={`rotate(-90 ${C} ${C})`}>
        <circle
          cx={C} cy={C} r={ARC_R}
          className="arc"
          pathLength={1000}
          style={{ strokeDasharray: `${f * 1000} 1000` }}
        />
      </g>
      <g className="bead" style={{ transform: `rotate(${f * 360}deg)` }}>
        <circle cx={C} cy={C - ARC_R} r={4.2} className="bead-dot" />
      </g>
      <circle cx={C} cy={C} r={91.5} fill="url(#glass)" pointerEvents="none" />
    </svg>
  );
}
