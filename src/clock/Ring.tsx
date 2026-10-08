// Minimal-style dial: a single thin progress ring, nothing else.

const C = 100;
const R = 88;

export function Ring({ fraction }: { fraction: number }) {
  const f = Math.min(1, Math.max(0, fraction));
  return (
    <svg className="dial ring" viewBox="0 0 200 200" aria-hidden>
      <circle cx={C} cy={C} r={R} className="ring-track" />
      <g transform={`rotate(-90 ${C} ${C})`}>
        <circle cx={C} cy={C} r={R} className="arc ring-arc" pathLength={1000} style={{ strokeDasharray: `${f * 1000} 1000` }} />
      </g>
    </svg>
  );
}
