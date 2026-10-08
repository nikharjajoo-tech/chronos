// The four widget layouts. Stage sizes (base px at 100% scale) must match
// WIDGETS in src-tauri/src/settings.rs.

import type { Widget } from "../types";
import {
  CapButtons,
  casingClass,
  Controls,
  DialUnit,
  Digits,
  MainKey,
  type Model,
  Pips,
  PLATE,
  subLabel,
  TaskLine,
  useWheelAdjust,
} from "./parts";

export const LAYOUTS: Record<Widget, { w: number; h: number }> = {
  pill: { w: 236, h: 68 },
  small: { w: 172, h: 172 },
  medium: { w: 340, h: 164 },
  large: { w: 240, h: 300 },
};

/** The full desk clock. */
function Large({ m }: { m: Model }) {
  return (
    <>
      <div className="feet" aria-hidden><i /><i /></div>
      <div className={casingClass(m)}>
        <header className="cap">
          <span className="lamp" title={m.snap.run} />
          <span className="brand">CHRONOS</span>
          <CapButtons m={m} />
        </header>
        <DialUnit m={m} size={166} hoverRow="nudges" />
        <TaskLine m={m} />
        <Controls m={m} />
      </div>
    </>
  );
}

/** Dial on the left; phase, task and keys on the right. */
function Medium({ m }: { m: Model }) {
  return (
    <div className={casingClass(m)}>
      <DialUnit m={m} size={128} plate={false} sub={false} hoverRow="nudges" />
      <div className="side">
        <div className="side-head">
          <span className="lamp" title={m.snap.run} />
          <span className="plate chip">{PLATE[m.snap.phase]}</span>
          <span className="grow" />
          <CapButtons m={m} soft />
        </div>
        <div className="side-meta">
          <Pips m={m} />
          <span className="sub-label">{subLabel(m.snap)}</span>
        </div>
        <TaskLine m={m} />
        <Controls m={m} />
      </div>
    </div>
  );
}

/** Just the dial. Keys appear on hover (and whenever the timer is idle). */
function Small({ m }: { m: Model }) {
  return (
    <div className={casingClass(m)}>
      <DialUnit m={m} size={140} hoverRow="controls" />
      <div className="corner">
        <CapButtons m={m} soft />
      </div>
    </div>
  );
}

/** A slim capsule: time, progress and one key. The least distracting option. */
function Pill({ m }: { m: Model }) {
  const onWheel = useWheelAdjust();
  return (
    <div className={casingClass(m)}>
      <span className="lamp" title={m.snap.run} />
      <div className="crt" onWheel={onWheel} title={PLATE[m.snap.phase]}>
        <Digits m={m} />
        <div className="bar">
          <i style={{ width: `${m.fraction * 100}%` }} />
        </div>
      </div>
      <MainKey m={m} round />
      <div className="pill-stack">
        <CapButtons m={m} soft />
      </div>
    </div>
  );
}

export function Layout({ m }: { m: Model }) {
  switch (m.settings.widget) {
    case "pill":
      return <Pill m={m} />;
    case "small":
      return <Small m={m} />;
    case "medium":
      return <Medium m={m} />;
    default:
      return <Large m={m} />;
  }
}
