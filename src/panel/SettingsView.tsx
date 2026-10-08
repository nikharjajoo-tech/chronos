import { type ReactNode, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { prettyShortcut, shortcutFromEvent } from "../format";
import { useSettings } from "../hooks";
import type { AppInfo, HelperStatus, Settings, Widget } from "../types";

const PRESETS = [
  { id: "standard", label: "Standard", sub: "25 / 5" },
  { id: "deep", label: "Deep Work", sub: "50 / 10" },
  { id: "quick", label: "Quick", sub: "15 / 5" },
];
const FOCUS_CHIPS = [15, 25, 30, 45, 50, 60];
const WIDGETS: { id: Widget; label: string; hint: string }[] = [
  { id: "pill", label: "Pill", hint: "time + one key" },
  { id: "small", label: "Small", hint: "dial only" },
  { id: "medium", label: "Medium", hint: "dial + details" },
  { id: "large", label: "Large", hint: "full desk clock" },
];
const isMac = navigator.platform.toLowerCase().includes("mac");

/** Friendly names for the built-in auto-hide process names. */
const KNOWN_APPS: Record<string, string> = {
  cpthost: "Zoom screen sharing",
  screencaptureui: "macOS screenshot & recording (⌘⇧5)",
  obs: "OBS Studio",
};
const AWAY_OPTIONS = [0, 3, 5, 10, 15, 20];

export function SettingsView() {
  const [settings, setLocal] = useSettings();
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const pending = useRef<number>(0);

  const [helpers, setHelpers] = useState<HelperStatus | null>(null);
  const [dndMsg, setDndMsg] = useState<string | null>(null);

  useEffect(() => {
    api.appInfo().then(setInfo);
    const refresh = () => api.helperStatus().then(setHelpers);
    refresh();
    // Coming back from the Shortcuts app re-checks whether they were added.
    window.addEventListener("focus", refresh);
    const id = window.setInterval(refresh, 5000);
    return () => {
      window.removeEventListener("focus", refresh);
      clearInterval(id);
    };
  }, []);

  if (!settings) return <div className="empty">Loading…</div>;

  /** Update locally at once; persist after a short pause (sliders fire a lot). */
  const save = (patch: Partial<Settings>, immediate = false) => {
    const next = { ...settings, ...patch };
    setLocal(next);
    clearTimeout(pending.current);
    const send = () =>
      api.setSettings(next).then((r) => {
        setLocal(r.settings);
        setShortcutError(r.shortcutError);
      });
    if (immediate) send();
    else pending.current = window.setTimeout(send, 250);
  };

  return (
    <div className="settings">
      <section className="card">
        <h3>Focus mode</h3>
        <div className="segmented">
          {PRESETS.map((p) => (
            <button key={p.id} className={settings.preset === p.id ? "on" : ""} onClick={() => api.act({ type: "preset", value: p.id })}>
              {p.label}
              <small>{p.sub}</small>
            </button>
          ))}
          <button className={settings.preset === "custom" ? "on" : ""} disabled>
            Custom
            <small>{settings.preset === "custom" ? `${settings.focusMin} / ${settings.shortMin}` : "edit below"}</small>
          </button>
        </div>

        <Row label="Focus">
          <div className="chips">
            {FOCUS_CHIPS.map((m) => (
              <button key={m} className={settings.focusMin === m ? "on" : ""} onClick={() => save({ focusMin: m }, true)}>
                {m}
              </button>
            ))}
          </div>
          <Minutes value={settings.focusMin} onChange={(v) => save({ focusMin: v })} />
        </Row>
        <Row label="Short break">
          <Minutes value={settings.shortMin} onChange={(v) => save({ shortMin: v })} />
        </Row>
        <Row label="Long break">
          <Minutes value={settings.longMin} onChange={(v) => save({ longMin: v })} />
        </Row>
        <Row label="Long break after">
          <Stepper value={settings.longEvery} min={1} max={12} onChange={(v) => save({ longEvery: v }, true)} suffix="focus sessions" />
        </Row>
        <Toggle label="Start breaks automatically" checked={settings.autoStartBreaks} onChange={(v) => save({ autoStartBreaks: v }, true)} />
        <Toggle label="Start next focus automatically" checked={settings.autoStartFocus} onChange={(v) => save({ autoStartFocus: v }, true)} />
        <p className="note">New durations apply to the next session. A session in progress keeps its time; use +5 / −5 or click the digits to change it.</p>
      </section>

      <section className="card">
        <h3>Focus helpers</h3>
        <Row label="Pause when I step away">
          <select value={settings.awayMin} onChange={(e) => save({ awayMin: +e.target.value }, true)}>
            {AWAY_OPTIONS.map((m) => (
              <option key={m} value={m}>
                {m === 0 ? "Never" : `after ${m} min idle`}
              </option>
            ))}
          </select>
        </Row>
        <p className="note">
          With no keyboard or mouse activity (or the lid closed), the focus session pauses as of the moment you left, so time away
          never counts as focus. Reading or watching without touching anything counts as idle too, so pick a longer time if needed.
        </p>
        <Toggle
          label="Do Not Disturb during focus"
          hint="On while a focus session runs, off on breaks and pauses."
          checked={settings.dnd}
          onChange={(v) => save({ dnd: v }, true)}
        />
        {settings.dnd && helpers && !helpers.dndInstalled && (
          <div className="row">
            <label className="muted">Needs two shortcuts in the Shortcuts app</label>
            <div className="row-controls">
              <button
                className="ghost"
                onClick={() => {
                  setDndMsg("Click “Add Shortcut” in both windows that open…");
                  api.installDndShortcuts().catch((e) => setDndMsg(String(e)));
                }}
              >
                Install shortcuts
              </button>
            </div>
          </div>
        )}
        {settings.dnd && helpers?.dndInstalled && <p className="note status-ok">✓ “Chronos Focus On” and “Chronos Focus Off” are installed.</p>}
        {settings.dnd && dndMsg && !helpers?.dndInstalled && <p className="note">{dndMsg}</p>}
      </section>

      <section className="card">
        <h3>Distractions & reflection</h3>
        <Row label="Log a distraction">
          <ShortcutRecorder value={settings.distractionShortcut} onChange={(v) => save({ distractionShortcut: v }, true)} />
        </Row>
        <p className="note">Works from any app. Logs the moment instantly; picking a reason is optional (keys 1–7).</p>
        <Toggle
          label="Quick reflection after focus sessions"
          hint="“How productive was it?” with an optional goal check. Always skippable."
          checked={settings.reflection}
          onChange={(v) => save({ reflection: v }, true)}
        />
        <Toggle
          label="Notifications"
          hint="When a session ends or is paused while you're away."
          checked={settings.notifications}
          onChange={(v) => save({ notifications: v }, true)}
        />
      </section>

      <section className="card">
        <h3>Widget</h3>
        <div className="widget-picker">
          {WIDGETS.map((w) => (
            <button key={w.id} className={settings.widget === w.id ? "on" : ""} onClick={() => save({ widget: w.id }, true)}>
              <span className={`wp-shape ${w.id}`} aria-hidden />
              <b>{w.label}</b>
              <small>{w.hint}</small>
            </button>
          ))}
        </div>
        <Row label="Style">
          <div className="chips">
            <button className={settings.style === "retro" ? "on" : ""} onClick={() => save({ style: "retro" }, true)}>Retro</button>
            <button className={settings.style === "minimal" ? "on" : ""} onClick={() => save({ style: "minimal" }, true)}>Minimal</button>
          </div>
        </Row>
        <Row label="Size">
          <input type="range" min={60} max={200} step={5} value={Math.round(settings.scale * 100)} onChange={(e) => save({ scale: +e.target.value / 100 })} />
          <span className="value">{Math.round(settings.scale * 100)}%</span>
        </Row>
        <p className="note">Also from the clock’s ⋯ menu → Widget Size, or drag its corner.</p>
      </section>

      <section className="card">
        <h3>Clock</h3>
        <Toggle label="Always on top" checked={settings.alwaysOnTop} onChange={(v) => save({ alwaysOnTop: v }, true)} />
        <Toggle label="Show on every desktop (Space)" checked={settings.allWorkspaces} onChange={(v) => save({ allWorkspaces: v }, true)} />
        <Row label="Countdown in menu bar">
          <div className="chips">
            {(
              [
                ["off", "Off"],
                ["hidden", "When hidden"],
                ["always", "Always"],
              ] as const
            ).map(([v, l]) => (
              <button key={v} className={settings.menuBarTimer === v ? "on" : ""} onClick={() => save({ menuBarTimer: v }, true)}>
                {l}
              </button>
            ))}
          </div>
        </Row>
        <Row label="Opacity">
          <input
            type="range"
            min={25}
            max={100}
            value={Math.round(settings.opacity * 100)}
            onChange={(e) => save({ opacity: +e.target.value / 100 })}
          />
          <span className="value">{Math.round(settings.opacity * 100)}%</span>
        </Row>
      </section>

      <section className="card">
        <h3>Hide mode</h3>
        <p className="note">
          Hiding the clock never pauses the timer. Sessions keep running and keep being recorded. Bring the clock back from the menu bar
          or with the shortcut.
        </p>
        <Row label="Hide / show shortcut">
          <ShortcutRecorder value={settings.hideShortcut} onChange={(v) => save({ hideShortcut: v }, true)} />
        </Row>
        {shortcutError && <p className="error">{shortcutError}</p>}
        <Toggle
          label="Hide automatically while sharing or recording"
          hint="Hides when any app below is running, and comes back when it quits."
          checked={settings.autoHide}
          onChange={(v) => save({ autoHide: v }, true)}
        />
        {settings.autoHide && (
          <AppList
            apps={settings.autoHideApps}
            running={helpers?.running ?? []}
            onChange={(autoHideApps) => save({ autoHideApps }, true)}
          />
        )}
        <Toggle
          label="Invisible in screenshots & screen recordings"
          hint="You still see the clock; captures don't. Some newer recorders ignore this, so use Hide when it matters."
          checked={settings.captureProtected}
          onChange={(v) => save({ captureProtected: v }, true)}
        />
      </section>

      <section className="card">
        <h3>Startup</h3>
        <Toggle
          label="Open Chronos at login"
          hint="Starts quietly with your Mac and puts the clock back where you left it."
          checked={settings.launchAtLogin}
          onChange={(v) => save({ launchAtLogin: v }, true)}
        />
        <Toggle
          label="Show in Dock"
          hint="Off keeps Chronos in the menu bar only. Either way, open it from Launchpad or Spotlight."
          checked={settings.showInDock}
          onChange={(v) => save({ showInDock: v }, true)}
        />
      </section>

      <section className="card">
        <h3>Sound</h3>
        <Toggle label="Chime when a session ends" checked={settings.sound} onChange={(v) => save({ sound: v }, true)} />
        <Row label="Volume">
          <input type="range" min={0} max={100} value={Math.round(settings.volume * 100)} disabled={!settings.sound} onChange={(e) => save({ volume: +e.target.value / 100 })} />
          <span className="value">{Math.round(settings.volume * 100)}%</span>
          <button className="ghost" onClick={() => api.testSound()}>Test</button>
        </Row>
      </section>

      <section className="card">
        <h3>Daily goal</h3>
        <Row label="Focus time per day">
          <Stepper
            value={settings.dailyGoalMin / 60}
            min={0}
            max={12}
            step={0.5}
            onChange={(v) => save({ dailyGoalMin: Math.round(v * 60) }, true)}
            suffix="hours"
          />
        </Row>
      </section>

      <section className="card">
        <h3>Your data</h3>
        <p className="note">
          Everything stays on this computer in a local SQLite database. There's no account and no network access.
        </p>
        {info && (
          <Row label="Location">
            <code className="path" title={info.dataDir}>
              {info.dataDir.length > 36 ? `…${info.dataDir.slice(-35)}` : info.dataDir}
            </code>
            <button className="ghost" onClick={() => api.revealDataDir()}>Reveal</button>
          </Row>
        )}
        {info && <p className="note muted">Chronos {info.version}</p>}
      </section>
    </div>
  );
}

function AppList({ apps, running, onChange }: { apps: string[]; running: string[]; onChange: (apps: string[]) => void }) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const name = draft.trim();
    if (name && !apps.some((a) => a.toLowerCase() === name.toLowerCase())) onChange([...apps, name]);
    setDraft("");
  };
  return (
    <>
      <div className="app-list">
        {apps.map((a) => {
          const live = running.some((r) => r.toLowerCase() === a.toLowerCase());
          return (
            <span key={a} className={live ? "app-chip live" : "app-chip"} title={live ? "Running now" : a}>
              {KNOWN_APPS[a.toLowerCase()] ?? a}
              {live && " ●"}
              <button onClick={() => onChange(apps.filter((x) => x !== a))} aria-label={`Remove ${a}`}>
                ×
              </button>
            </span>
          );
        })}
      </div>
      <div className="row">
        <label className="muted">Add an app (e.g. Keynote, Microsoft Teams)</label>
        <div className="row-controls app-add">
          <input value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} placeholder="App name" />
          <button className="ghost" onClick={add} disabled={!draft.trim()}>
            Add
          </button>
        </div>
      </div>
      <p className="note">
        Browser-based calls (Google Meet in Chrome) can't be detected, so use the hide shortcut or turn on “Invisible in screenshots &
        recordings” for those.
      </p>
    </>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="row">
      <label>{label}</label>
      <div className="row-controls">{children}</div>
    </div>
  );
}

function Toggle({ label, hint, checked, onChange }: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <span>
        {label}
        {hint && <small>{hint}</small>}
      </span>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <i aria-hidden />
    </label>
  );
}

function Minutes({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  return (
    <span className="minutes">
      <input
        type="number"
        min={1}
        max={240}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          const v = Math.round(+e.target.value);
          if (v >= 1 && v <= 240) onChange(v);
        }}
        onBlur={() => setDraft(String(value))}
      />
      min
    </span>
  );
}

function Stepper({ value, min, max, step = 1, suffix, onChange }: { value: number; min: number; max: number; step?: number; suffix?: string; onChange: (v: number) => void }) {
  const set = (v: number) => onChange(Math.min(max, Math.max(min, v)));
  return (
    <span className="stepper">
      <button onClick={() => set(value - step)} disabled={value <= min} aria-label="Less">−</button>
      <b>{value}</b>
      <button onClick={() => set(value + step)} disabled={value >= max} aria-label="More">+</button>
      {suffix && <span className="muted">{suffix}</span>}
    </span>
  );
}

function ShortcutRecorder({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [recording, setRecording] = useState(false);
  return (
    <button
      className={recording ? "shortcut recording" : "shortcut"}
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={(e) => {
        if (!recording) return;
        e.preventDefault();
        if (e.key === "Escape") return setRecording(false);
        const s = shortcutFromEvent(e.nativeEvent, isMac);
        if (s) {
          onChange(s);
          setRecording(false);
        }
      }}
    >
      {recording ? "Press keys…" : prettyShortcut(value)}
    </button>
  );
}
