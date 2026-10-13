import { useEffect, useState } from "react";
import { api } from "../api";
import type { InsightsView, Score, Suggestion } from "../types";

export function Insights() {
  const [view, setView] = useState<InsightsView | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    const load = () => api.insights().then((v) => live && setView(v));
    load();
    const offStats = api.onStats(load);
    const id = window.setInterval(load, 60_000);
    return () => {
      live = false;
      offStats();
      clearInterval(id);
    };
  }, []);

  if (!view) return <div className="empty">Loading…</div>;

  const respond = async (s: Suggestion, accept: boolean) => {
    setBusy(true);
    setView(await api.respondSuggestion(s.id, accept));
    setBusy(false);
  };

  return (
    <div className="insights">
      <header className="hist-head">
        <div className="td-title">
          <h1>Insights</h1>
          <span className="td-date">From your last 4 weeks of focus</span>
        </div>
      </header>

      <ScoreCard view={view} />

      {view.suggestion && (
        <section className="card suggestion">
          <h3>A suggestion</h3>
          <p className="sugg-text">{view.suggestion.text}</p>
          <p className="evidence">{view.suggestion.evidence}</p>
          <div className="sugg-actions">
            {view.suggestion.action && (
              <button className="primary" disabled={busy} onClick={() => respond(view.suggestion!, true)}>
                {view.suggestion.action.label}
              </button>
            )}
            <button className="ghost" disabled={busy} onClick={() => respond(view.suggestion!, false)}>
              {view.suggestion.action ? "No thanks" : "Got it"}
            </button>
          </div>
        </section>
      )}

      <section className="card">
        <h3>What your data says</h3>
        {view.insights.length === 0 ? (
          <div className="empty">
            Nothing to report yet. Insights appear once there's enough data to back them up, usually after a week or two of sessions
            ({view.sample} so far in the last 4 weeks).
          </div>
        ) : (
          <ul className="insight-list">
            {view.insights.map((i) => (
              <li key={i.id}>
                <p>{i.text}</p>
                <p className="evidence">{i.evidence}</p>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function ScoreCard({ view }: { view: InsightsView }) {
  const { score, previous, mover } = view;
  if (!score) {
    return (
      <section className="card score-card">
        <h3>This week’s score</h3>
        <div className="empty">Finish a focus session this week to get a score.</div>
        <Trend view={view} />
      </section>
    );
  }
  const delta = previous && previous.value > 0 ? (score.value - previous.value) / previous.value : null;
  return (
    <section className="card score-card">
      <h3>This week’s score</h3>
      <div className="score-top">
        <div className="score-num">
          {score.value}
          <span>/100</span>
        </div>
        <div className="score-why">
          {delta !== null && Math.abs(delta) >= 0.01 ? (
            <div className={`delta big ${delta > 0 ? "good" : "bad"}`}>
              {delta > 0 ? "↑" : "↓"} {Math.round(Math.abs(delta) * 100)}% vs the same days last week
            </div>
          ) : previous ? (
            <div className="muted">About the same as the same days last week</div>
          ) : (
            <div className="muted">No sessions in the same days last week to compare with</div>
          )}
          {mover && (
            <div className="muted">
              Mostly from <b>{mover[0].toLowerCase()}</b> ({mover[1] > 0 ? "+" : ""}
              {Math.round(mover[1])} pts)
            </div>
          )}
        </div>
      </div>
      <Parts score={score} />
      <Trend view={view} />
    </section>
  );
}

/** Each part: how many of its points you got, and the evidence. */
function Parts({ score }: { score: Score }) {
  return (
    <ul className="parts">
      {score.parts.map((p) => (
        <li key={p.key} title={`${Math.round(p.points)} of ${Math.round(p.max)} points`}>
          <span className="part-name">
            {p.label}
            <span className="part-detail">{p.detail}</span>
          </span>
          <span className="part-track">
            <i style={{ width: `${(p.points / p.max) * 100}%` }} />
          </span>
          <span className="part-pts">
            {Math.round(p.points)}
            <small>/{Math.round(p.max)}</small>
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Last 8 weeks of scores. One series, so the heading names it; no legend. */
function Trend({ view }: { view: InsightsView }) {
  const [hover, setHover] = useState<number | null>(null);
  const W = 560;
  const H = 96;
  const base = H - 18;
  const slot = W / view.trend.length;
  const bw = Math.min(40, slot - 8);
  const y = (v: number) => base - (v / 100) * (base - 6);
  const label = (t: number) => new Date(t).toLocaleDateString([], { day: "numeric", month: "short" });
  return (
    <div className="trend" onMouseLeave={() => setHover(null)}>
      <div className="trend-title">Last 8 weeks</div>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="Weekly score, last 8 weeks">
        <line x1={0} x2={W} y1={y(50)} y2={y(50)} className="grid-line" />
        {view.trend.map((w, i) => {
          const x = i * slot + (slot - bw) / 2;
          const last = i === view.trend.length - 1;
          const h = w.value !== null ? base - y(w.value) : 0;
          const r = Math.min(4, h);
          return (
            <g key={w.start} onMouseEnter={() => setHover(i)}>
              <rect x={i * slot} y={0} width={slot} height={H} fill="transparent" />
              {w.value === null ? (
                <rect x={x} y={base - 1.5} width={bw} height={1.5} className="bar-zero" />
              ) : (
                <path
                  d={`M${x},${base} V${base - h + r} Q${x},${base - h} ${x + r},${base - h} H${x + bw - r} Q${x + bw},${base - h} ${x + bw},${base - h + r} V${base} Z`}
                  className={`bar ${last ? "current" : ""} ${hover === i ? "on" : ""}`}
                />
              )}
              {(last || hover === i) && w.value !== null && (
                <text x={x + bw / 2} y={base - h - 5} textAnchor="middle" className="axis-text strong">
                  {w.value}
                </text>
              )}
              <text x={x + bw / 2} y={H - 3} textAnchor="middle" className="axis-text">
                {last ? "This week" : i % 2 === 1 ? label(w.start) : ""}
              </text>
            </g>
          );
        })}
        <line x1={0} x2={W} y1={base} y2={base} className="base-line" />
      </svg>
      {hover !== null && (
        <div className="tip" style={{ left: `${((hover + 0.5) / view.trend.length) * 100}%` }}>
          <b>Week of {label(view.trend[hover].start)}</b>
          <div>{view.trend[hover].value === null ? "No sessions" : `Score ${view.trend[hover].value}`}</div>
        </div>
      )}
    </div>
  );
}
