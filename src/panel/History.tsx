import { useEffect, useState } from "react";
import { api } from "../api";
import { fmtDuration, prettyShortcut, ratingFace } from "../format";
import { useSettings } from "../hooks";
import { DISTRACTION_CATEGORIES, type Named, type PeriodStats } from "../types";
import { Figure, Projects } from "./Today";

type Kind = "week" | "month";
const DAY_MS = 86_400_000;

export function History() {
  const [kind, setKind] = useState<Kind>("week");
  const [offset, setOffset] = useState(0);
  const [data, setData] = useState<PeriodStats | null>(null);
  const [settings] = useSettings();

  useEffect(() => {
    let live = true;
    const load = () => api.periodStats(kind, offset).then((d) => live && setData(d));
    load();
    const offStats = api.onStats(load);
    const offSnap = api.onSnapshot(load);
    return () => {
      live = false;
      offStats();
      offSnap();
    };
  }, [kind, offset]);

  if (!data) return <div className="empty">Loading…</div>;
  const t = data.totals;
  const p = data.previous;
  const current = offset === 0;
  const vs = current ? `vs same days last ${kind}` : `vs previous ${kind}`;

  const extras = [
    t.focusSessions > 0 && `avg session ${fmtDuration(t.avgSessionMs)}`,
    t.focusSessions > 0 && `${Math.round(t.completionRate * 100)}% finished`,
    `focus on ${t.activeDays} of ${t.elapsedDays} ${t.elapsedDays === 1 ? "day" : "days"}`,
    data.streak.current > 0 && `${data.streak.current}-day streak (best ${data.streak.best})`,
    t.avgRating && `rating ${ratingFace(t.avgRating)} ${t.avgRating.toFixed(1)}/4`,
  ].filter((e): e is string => !!e);

  return (
    <div className="history">
      <header className="hist-head">
        <div className="td-title">
          <h1>{periodLabel(kind, data, offset)}</h1>
          <span className="td-date">{rangeLabel(data)}</span>
        </div>
        <div className="hist-controls">
          <div className="segmented small">
            {(["week", "month"] as Kind[]).map((k) => (
              <button
                key={k}
                className={kind === k ? "on" : ""}
                onClick={() => {
                  setKind(k);
                  setOffset(0);
                }}
              >
                {k === "week" ? "Week" : "Month"}
              </button>
            ))}
          </div>
          <div className="period-nav">
            <button onClick={() => setOffset(offset - 1)} aria-label={`Previous ${kind}`}>‹</button>
            <button onClick={() => setOffset(offset + 1)} disabled={current} aria-label={`Next ${kind}`}>›</button>
          </div>
        </div>
      </header>

      <section className="card td-figures">
        <div className="td-figs">
          <Figure value={fmtDuration(t.focusMs)} label="focus time" delta={<Delta value={change(t.focusMs, p.focusMs)} vs={vs} />} />
          <Figure value={fmtDuration(t.avgDailyFocusMs)} label="per day" delta={<Delta value={change(t.avgDailyFocusMs, p.avgDailyFocusMs)} vs={vs} />} />
          <Figure value={t.completed} label="completed" delta={<Delta value={change(t.completed, p.completed)} vs={vs} />} />
          <Figure
            value={t.distractions}
            label={t.distractions === 1 ? "distraction" : "distractions"}
            delta={<Delta value={change(t.distractions, p.distractions)} vs={vs} lowerIsBetter />}
          />
        </div>
        <div className="td-extras">
          <div>
            <div>
              {extras.map((e) => (
                <span key={e}>{e}</span>
              ))}
            </div>
          </div>
        </div>
      </section>

      <section className="card">
        <h3>Focus per day</h3>
        <DayBars data={data} kind={kind} goalMs={(settings?.dailyGoalMin ?? 0) * 60_000} />
      </section>

      <section className="card">
        <h3>When you focus</h3>
        <HourStrip label="Focus" values={data.focusByHour} format={(v) => fmtDuration(v)} tone="focus" />
        <HourStrip label="Distractions" values={data.distractionsByHour} format={(v) => `${v}`} tone="distraction" />
        <p className="note">{hourInsight(data)}</p>
      </section>

      <div className="two-col">
        <section className="card">
          <h3>Projects</h3>
          <Projects rows={data.projects} />
        </section>
        <section className="card">
          <h3>Distractions</h3>
          <Bars
            rows={data.categories}
            empty={`Log one with ${prettyShortcut(settings?.distractionShortcut ?? "Control+Alt+D")} when something pulls you away.`}
            format={(v) => `${v}`}
            label={(n) => DISTRACTION_CATEGORIES.find((c) => c.id === n)?.label ?? "Not specified"}
            tone="distraction"
          />
        </section>
      </div>
    </div>
  );
}

function periodLabel(kind: Kind, d: PeriodStats, offset: number): string {
  const from = new Date(d.from);
  if (kind === "month") return from.toLocaleDateString([], { month: "long", year: "numeric" });
  if (offset === 0) return "This week";
  if (offset === -1) return "Last week";
  const to = new Date(d.to - DAY_MS);
  const f = (x: Date) => x.toLocaleDateString([], { day: "numeric", month: "short" });
  return `${f(from)} – ${f(to)}`;
}

function rangeLabel(d: PeriodStats): string {
  const f = (t: number) => new Date(t).toLocaleDateString([], { day: "numeric", month: "short" });
  return `${f(d.from)} – ${f(d.to - DAY_MS)}`;
}

/** Fractional change, or null when there's nothing to compare against. */
function change(now: number, before: number): number | null {
  if (before <= 0) return null;
  return (now - before) / before;
}

/** "↑ 22%" against the previous period; green when it moved the right way. */
function Delta({ value, vs, lowerIsBetter }: { value: number | null; vs: string; lowerIsBetter?: boolean }) {
  if (value === null || Math.abs(value) < 0.01) return null;
  const good = lowerIsBetter ? value < 0 : value > 0;
  return (
    <span className={`delta ${good ? "good" : "bad"}`} title={vs}>
      {value > 0 ? "↑" : "↓"} {Math.round(Math.abs(value) * 100)}%
    </span>
  );
}

/** One bar per day. Single series, so no legend; the title names it. */
function DayBars({ data, kind, goalMs }: { data: PeriodStats; kind: Kind; goalMs: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const days = data.days;
  const max = Math.max(goalMs, ...days.map((d) => d.focusMs), 60 * 60_000);
  const W = 600;
  const H = 150;
  const top = 8;
  const base = H - 22;
  const slot = W / days.length;
  const bw = Math.max(4, Math.min(42, slot - 2));
  const y = (v: number) => base - (v / max) * (base - top);
  const today = new Date();
  today.setHours(0, 0, 0, 0);

  const label = (start: number, i: number) => {
    const d = new Date(start);
    if (kind === "week") return d.toLocaleDateString([], { weekday: "short" });
    return i === 0 || (d.getDate() % 5 === 0 && d.getDate() < 30) ? `${d.getDate()}` : "";
  };

  const h = hover !== null ? days[hover] : null;
  return (
    <div className="daybars" onMouseLeave={() => setHover(null)}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Focus time per day">
        {[0.5, 1].map((f) => (
          <line key={f} x1={0} x2={W} y1={y(max * f)} y2={y(max * f)} className="grid-line" />
        ))}
        {goalMs > 0 && (
          <g>
            <line x1={0} x2={W} y1={y(goalMs)} y2={y(goalMs)} className="goal-line" />
            <text x={W - 2} y={y(goalMs) - 4} textAnchor="end" className="axis-text">
              goal {fmtDuration(goalMs)}
            </text>
          </g>
        )}
        {days.map((d, i) => {
          const x = i * slot + (slot - bw) / 2;
          const height = Math.max(0, base - y(d.focusMs));
          const r = Math.min(4, bw / 2, height);
          return (
            <g key={d.start} onMouseEnter={() => setHover(i)}>
              <rect x={i * slot} y={0} width={slot} height={H} fill="transparent" />
              {d.future ? (
                <rect x={x} y={base - 3} width={bw} height={3} rx={1.5} className="bar-future" />
              ) : height > 0 ? (
                <path
                  d={`M${x},${base} V${base - height + r} Q${x},${base - height} ${x + r},${base - height} H${x + bw - r} Q${x + bw},${base - height} ${x + bw},${base - height + r} V${base} Z`}
                  className={hover === i ? "bar on" : "bar"}
                />
              ) : (
                <rect x={x} y={base - 1.5} width={bw} height={1.5} className="bar-zero" />
              )}
              {d.start === today.getTime() && <circle cx={x + bw / 2} cy={base + 6} r={2} className="today-dot" />}
              <text x={x + bw / 2} y={H - 3} textAnchor="middle" className="axis-text">
                {label(d.start, i)}
              </text>
            </g>
          );
        })}
        <line x1={0} x2={W} y1={base} y2={base} className="base-line" />
      </svg>
      {h && hover !== null && (
        <div className="tip" style={{ left: `${((hover + 0.5) / days.length) * 100}%` }}>
          <b>{new Date(h.start).toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}</b>
          {h.future ? (
            <div>Still to come</div>
          ) : (
            <>
              <div>{fmtDuration(h.focusMs)} focused</div>
              <div>
                {h.completed} completed{h.abandoned ? ` · ${h.abandoned} ended early` : ""}
              </div>
              <div>{h.distractions} distractions</div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** 6 AM – midnight as a strip of cells, shaded by one hue (more = darker). */
function HourStrip({ label, values, format, tone }: { label: string; values: number[]; format: (v: number) => string; tone: "focus" | "distraction" }) {
  const hours = Array.from({ length: 18 }, (_, i) => i + 6);
  const max = Math.max(1, ...hours.map((h) => values[h] ?? 0));
  return (
    <div className="hourstrip">
      <span className="hs-label">{label}</span>
      <div className="hs-cells">
        {hours.map((h) => {
          const v = values[h] ?? 0;
          return (
            <i
              key={h}
              className={`hs-cell ${tone}`}
              style={{ ["--a" as string]: v === 0 ? 0 : 0.15 + 0.85 * (v / max) }}
              title={`${hourName(h)}–${hourName(h + 1)}: ${format(v)}`}
            />
          );
        })}
      </div>
      {label === "Distractions" && (
        <div className="hs-axis">
          {hours.filter((h) => h % 3 === 0).map((h) => (
            <span key={h} style={{ left: `${((h - 6) / 18) * 100}%` }}>
              {hourName(h)}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

const hourName = (h: number) => {
  const hh = h % 24;
  return hh === 0 ? "12a" : hh < 12 ? `${hh}a` : hh === 12 ? "12p" : `${hh - 12}p`;
};

/** Best 2-hour window for focus and for distractions, from the data. */
function hourInsight(d: PeriodStats): string {
  const best = (v: number[]) => {
    let at = -1;
    let top = 0;
    for (let h = 0; h < 23; h++) {
      const s = (v[h] ?? 0) + (v[h + 1] ?? 0);
      if (s > top) {
        top = s;
        at = h;
      }
    }
    return at;
  };
  const f = best(d.focusByHour);
  if (f < 0) return "Finish a few sessions to see when you focus best.";
  const parts = [`Most of your focus happens ${hourName(f)}–${hourName(f + 2)}.`];
  const x = best(d.distractionsByHour);
  if (x >= 0 && d.totals.distractions >= 3) parts.push(`Distractions peak ${hourName(x)}–${hourName(x + 2)}.`);
  return parts.join(" ");
}

export function Bars({ rows, format, label, empty, tone = "focus" }: { rows: Named[]; format: (v: number) => string; label: (name: string) => string; empty: string; tone?: "focus" | "distraction" }) {
  if (rows.length === 0) return <div className="empty">{empty}</div>;
  const total = rows.reduce((a, r) => a + r.value, 0);
  const max = Math.max(...rows.map((r) => r.value));
  return (
    <ul className="hbars">
      {rows.slice(0, 6).map((r) => (
        <li key={r.name} title={`${label(r.name)}: ${format(r.value)} (${Math.round((r.value / total) * 100)}%)`}>
          <span className="hb-name">{label(r.name)}</span>
          <span className="hb-track">
            <i className={tone} style={{ width: `${(r.value / max) * 100}%` }} />
          </span>
          <span className="hb-value">{format(r.value)}</span>
        </li>
      ))}
    </ul>
  );
}

