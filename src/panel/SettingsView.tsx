import { type ReactNode, useEffect, useId, useRef, useState } from "react";
import { api } from "../api";
import { prettyShortcut, shortcutFromEvent } from "../format";
import { useSettings } from "../hooks";
import type { AppInfo, EmailStatus, HelperStatus, Settings, Widget } from "../types";

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

/** One section shows at a time; the last one picked comes back next time. */
const SECTIONS = [
  { id: "timer", label: "Timer" },
  { id: "focus", label: "Focus" },
  { id: "look", label: "Look" },
  { id: "hide", label: "Hiding" },
  { id: "general", label: "General" },
] as const;
type Section = (typeof SECTIONS)[number]["id"];
const SECTION_KEY = "chronos.settingsSection";
const savedSection = (): Section => {
  try {
    const v = localStorage.getItem(SECTION_KEY);
    return SECTIONS.some((x) => x.id === v) ? (v as Section) : "timer";
  } catch {
    return "timer";
  }
};

export function SettingsView() {
  const [settings, setLocal] = useSettings();
  const [section, setSection] = useState<Section>(savedSection);
  const pick = (id: Section) => {
    setSection(id);
    try {
      localStorage.setItem(SECTION_KEY, id);
    } catch {}
  };
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
      <header className="hist-head">
        <div className="td-title">
          <h1>Settings</h1>
        </div>
      </header>
      <nav className="set-nav" aria-label="Settings sections">
        {SECTIONS.map((x) => (
          <button key={x.id} className={section === x.id ? "on" : ""} aria-current={section === x.id} onClick={() => pick(x.id)}>
            {x.label}
          </button>
        ))}
      </nav>
      {shortcutError && <p className="error set-error">{shortcutError}</p>}

      {section === "timer" && (
        <>
          <Group title="Focus mode">
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
            <Row label="Focus" hint="New durations apply to the next session. To change the one running now, use +5 / −5 or click the digits on the clock.">
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
              <Stepper value={settings.longEvery} min={1} max={12} onChange={(v) => save({ longEvery: v }, true)} suffix="sessions" />
            </Row>
            <Toggle label="Start breaks automatically" checked={settings.autoStartBreaks} onChange={(v) => save({ autoStartBreaks: v }, true)} />
            <Toggle label="Start next focus automatically" checked={settings.autoStartFocus} onChange={(v) => save({ autoStartFocus: v }, true)} />
          </Group>

          <Group title="Daily goal">
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
          </Group>

          <Group title="Sound">
            <Toggle label="Chime when a session ends" checked={settings.sound} onChange={(v) => save({ sound: v }, true)} />
            <Row label="Volume">
              <input type="range" min={0} max={100} value={Math.round(settings.volume * 100)} disabled={!settings.sound} onChange={(e) => save({ volume: +e.target.value / 100 })} />
              <span className="value">{Math.round(settings.volume * 100)}%</span>
              <button className="ghost" onClick={() => api.testSound()}>Test</button>
            </Row>
          </Group>
        </>
      )}

      {section === "focus" && (
        <>
          <Group title="Focus helpers">
            <Row
              label="Pause when I step away"
              hint="With no keyboard or mouse activity (or the lid closed), focus pauses from the moment you left, so time away never counts. Reading without touching anything counts as idle too."
            >
              <select value={settings.awayMin} onChange={(e) => save({ awayMin: +e.target.value }, true)}>
                {AWAY_OPTIONS.map((m) => (
                  <option key={m} value={m}>
                    {m === 0 ? "Never" : `After ${m} min`}
                  </option>
                ))}
              </select>
            </Row>
            <Toggle
              label="Do Not Disturb during focus"
              hint="On while a focus session runs, off on breaks and pauses."
              checked={settings.dnd}
              onChange={(v) => save({ dnd: v }, true)}
            />
            {settings.dnd && helpers && !helpers.dndInstalled && (
              <Row label="Needs two Shortcuts" muted hint={dndMsg ?? "Chronos switches Focus through two shortcuts in the Shortcuts app."}>
                <button
                  className="ghost"
                  onClick={() => {
                    setDndMsg("Click “Add Shortcut” in both windows that open.");
                    api.installDndShortcuts().catch((e) => setDndMsg(String(e)));
                  }}
                >
                  Install
                </button>
              </Row>
            )}
            {settings.dnd && helpers?.dndInstalled && (
              <Row label="Shortcuts" muted>
                <span className="status-ok">✓ Installed</span>
              </Row>
            )}
          </Group>

          <Group title="Planning">
            <Row label="Quick add a task" hint="Opens a small box over any app. Enter adds to Today; ⌘Enter adds it and starts focus.">
              <ShortcutRecorder value={settings.quickAddShortcut} onChange={(v) => save({ quickAddShortcut: v }, true)} />
            </Row>
            <Toggle
              label="Morning planning"
              hint="The first time you're at your Mac each day, Plan opens so you can pick what goes in Today."
              checked={settings.morningPrompt}
              onChange={(v) => save({ morningPrompt: v }, true)}
            />
            {settings.morningPrompt && (
              <Row label="Not before" muted>
                <select value={settings.morningAfterHour} onChange={(e) => save({ morningAfterHour: +e.target.value }, true)}>
                  {Array.from({ length: 13 }, (_, h) => (
                    <option key={h} value={h}>
                      {h === 0 ? "Midnight" : h === 12 ? "12 PM" : `${h} AM`}
                    </option>
                  ))}
                </select>
              </Row>
            )}
          </Group>

          <Group title="Distractions & nudges">
            <Row label="Log a distraction" hint="Works from any app and logs the moment at once. Picking a reason (keys 1–7) is optional.">
              <ShortcutRecorder value={settings.distractionShortcut} onChange={(v) => save({ distractionShortcut: v }, true)} />
            </Row>
            <Toggle
              label="Reflection after each session"
              hint="“How productive was it?” with an optional goal check. Always skippable."
              checked={settings.reflection}
              onChange={(v) => save({ reflection: v }, true)}
            />
            <Toggle
              label="Companion messages"
              hint="A rare line on the clock, e.g. after a long run without a break. At most one every 90 minutes."
              checked={settings.companion}
              onChange={(v) => save({ companion: v }, true)}
            />
            <Toggle
              label="Notifications"
              hint="When a session ends, or is paused while you're away."
              checked={settings.notifications}
              onChange={(v) => save({ notifications: v }, true)}
            />
          </Group>
        </>
      )}

      {section === "look" && (
        <>
          <Group title="Panel">
            <Row label="Theme" hint="For this window and the board. Light or dark follows your Mac.">
              <div className="chips">
                <button className={settings.theme === "glass" ? "on" : ""} onClick={() => save({ theme: "glass" }, true)}>Glass</button>
                <button className={settings.theme === "retro" ? "on" : ""} onClick={() => save({ theme: "retro" }, true)}>Retro</button>
              </div>
            </Row>
          </Group>

          <Group title="Clock">
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
                <button className={settings.style === "glass" ? "on" : ""} onClick={() => save({ style: "glass" }, true)}>Glass</button>
              </div>
            </Row>
            <Row label="Size" hint="Also from the clock’s ⋯ menu → Widget Size, or drag its corner.">
              <input type="range" min={60} max={200} step={5} value={Math.round(settings.scale * 100)} onChange={(e) => save({ scale: +e.target.value / 100 })} />
              <span className="value">{Math.round(settings.scale * 100)}%</span>
            </Row>
            <Row label="Opacity">
              <input type="range" min={25} max={100} value={Math.round(settings.opacity * 100)} onChange={(e) => save({ opacity: +e.target.value / 100 })} />
              <span className="value">{Math.round(settings.opacity * 100)}%</span>
            </Row>
            <Toggle label="Always on top" checked={settings.alwaysOnTop} onChange={(v) => save({ alwaysOnTop: v }, true)} />
            <Toggle label="Show on every desktop" checked={settings.allWorkspaces} onChange={(v) => save({ allWorkspaces: v }, true)} />
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
          </Group>
        </>
      )}

      {section === "hide" && (
        <Group title="Hiding the clock">
          <Row label="Hide / show" hint="Hiding never pauses the timer: sessions keep running and being recorded. Bring the clock back from the menu bar or with this shortcut.">
            <ShortcutRecorder value={settings.hideShortcut} onChange={(v) => save({ hideShortcut: v }, true)} />
          </Row>
          <Toggle
            label="Hide while sharing your screen"
            hint="Hides while any app below is running and comes back when it quits."
            checked={settings.autoHide}
            onChange={(v) => save({ autoHide: v }, true)}
          />
          {settings.autoHide && (
            <AppList apps={settings.autoHideApps} running={helpers?.running ?? []} onChange={(autoHideApps) => save({ autoHideApps }, true)} />
          )}
          <Toggle
            label="Invisible in screenshots"
            hint="You still see the clock; screenshots and recordings don't. Some newer recorders ignore this, so use Hide when it matters."
            checked={settings.captureProtected}
            onChange={(v) => save({ captureProtected: v }, true)}
          />
        </Group>
      )}

      {section === "general" && (
        <>
          <Group title="Startup">
            <Toggle
              label="Open at login"
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
          </Group>

          <WeeklyEmail settings={settings} save={save} />

          <Group title="Your data">
            {info && (
              <Row label="Stored on this Mac" hint="Everything stays in a local SQLite database. There's no account; only the weekly email, if you turn it on, leaves your Mac.">
                <code className="path" title={info.dataDir}>
                  {info.dataDir.length > 30 ? `…${info.dataDir.slice(-29)}` : info.dataDir}
                </code>
                <button className="ghost" onClick={() => api.revealDataDir()}>Reveal</button>
              </Row>
            )}
            {info && (
              <Row label="Version" muted>
                <span className="value">{info.version}</span>
              </Row>
            )}
          </Group>
        </>
      )}
    </div>
  );
}

const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const hourLabel = (h: number) => new Date(2000, 0, 1, h).toLocaleTimeString([], { hour: "numeric" });

function WeeklyEmail({ settings, save }: { settings: Settings; save: (p: Partial<Settings>, immediate?: boolean) => void }) {
  const [status, setStatus] = useState<EmailStatus | null>(null);
  const [pw, setPw] = useState("");
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = () => api.emailStatus().then(setStatus);
  useEffect(() => {
    refresh();
    return api.onStats(refresh);
  }, [settings.emailFrom]);

  const run = async (fn: () => Promise<string | void>, okText?: string) => {
    setBusy(true);
    setMsg(null);
    try {
      const r = await fn();
      setMsg({ ok: true, text: (r as string) || okText || "Done." });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    }
    setBusy(false);
    refresh();
  };

  const st = status?.status;
  return (
    <Group title="Weekly email">
      <Toggle
        label="Weekly summary email"
        hint="Score, totals against the week before, focus per day, projects, insights and one suggestion. Never task names. The only thing that leaves your Mac, and only while this is on."
        checked={settings.emailReport}
        onChange={(v) => save({ emailReport: v }, true)}
      />
      {settings.emailReport && (
        <>
          <Row label="Send to">
            <input className="text" type="email" placeholder="you@example.com" value={settings.emailTo} onChange={(e) => save({ emailTo: e.target.value })} />
          </Row>
          <Row label="Send from (Gmail)">
            <input className="text" type="email" placeholder="you@gmail.com" value={settings.emailFrom} onChange={(e) => save({ emailFrom: e.target.value })} />
          </Row>
          <Row label="App password" hint="Gmail needs an app password, not your normal one (it requires 2-Step Verification). Name it “Chronos” and paste the 16 letters. It's kept only in the macOS Keychain.">
            {status?.hasPassword && !pw ? <span className="status-ok">✓ In Keychain</span> : null}
            <input
              className="text short"
              type="password"
              placeholder={status?.hasPassword ? "Replace…" : "abcd efgh ijkl mnop"}
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              autoComplete="off"
            />
            <button className="ghost" disabled={!pw || !settings.emailFrom || busy} onClick={() => run(() => api.setEmailPassword(pw).then(() => setPw("")), "Saved in your Keychain.")}>
              Save
            </button>
          </Row>
          <Row label="No app password yet?" muted>
            <button className="link" onClick={() => api.openAppPasswords()}>Create one at Google →</button>
          </Row>
          <Row label="When" hint="If your Mac is off at send time, it goes out at the next chance; a failed send retries every 30 minutes.">
            <select value={settings.emailDay} onChange={(e) => save({ emailDay: +e.target.value }, true)}>
              {DAYS.map((d, i) => (
                <option key={d} value={i}>{d}s</option>
              ))}
            </select>
            <select value={settings.emailHour} onChange={(e) => save({ emailHour: +e.target.value }, true)}>
              {Array.from({ length: 24 }, (_, h) => (
                <option key={h} value={h}>{hourLabel(h)}</option>
              ))}
            </select>
          </Row>
          <div className="row">
            <label className="muted">
              {st?.lastError ? (
                <span className="error-inline">Last attempt failed: {st.lastError}</span>
              ) : st?.lastSentAt ? (
                `Last sent ${new Date(st.lastSentAt).toLocaleString([], { weekday: "short", day: "numeric", month: "short", hour: "numeric", minute: "2-digit" })}`
              ) : status ? (
                `Next: ${new Date(status.nextSend).toLocaleString([], { weekday: "long", hour: "numeric", minute: "2-digit" })}`
              ) : null}
            </label>
            <div className="row-controls">
              <button className="ghost" disabled={busy} onClick={() => run(() => api.previewEmail(), "Opened the preview in your browser.")}>
                Preview
              </button>
              <button className="ghost" disabled={busy || !status?.hasPassword || !settings.emailTo} onClick={() => run(() => api.sendTestEmail())}>
                {busy ? "Sending…" : "Send test"}
              </button>
            </div>
          </div>
          {msg && <p className={msg.ok ? "note status-ok" : "error"}>{msg.text}</p>}
        </>
      )}
    </Group>
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
      <Row
        label="Add an app"
        muted
        hint="Browser calls (Google Meet in Chrome) can't be detected; use the hide shortcut or “Invisible in screenshots” for those."
      >
        <div className="app-add">
          <input value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} placeholder="e.g. Keynote, Microsoft Teams" />
          <button className="ghost" onClick={add} disabled={!draft.trim()}>
            Add
          </button>
        </div>
      </Row>
    </>
  );
}

/** A titled group of one-line rows. */
function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="set-group">
      <h3>{title}</h3>
      <div className="set-list">{children}</div>
    </section>
  );
}

/** ⓘ after a label: the details show on hover or keyboard focus. */
function Info({ text }: { text: string }) {
  return (
    <button type="button" className="info" aria-label={text} onClick={(e) => e.preventDefault()}>
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden>
        <circle cx="7" cy="7" r="6" fill="none" stroke="currentColor" strokeWidth="1.2" />
        <path d="M7 6.2v3.6M7 4.2v.1" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
      </svg>
      <span className="info-tip" aria-hidden>
        {text}
      </span>
    </button>
  );
}

function Row({ label, hint, muted, children }: { label: string; hint?: string; muted?: boolean; children: ReactNode }) {
  return (
    <div className="row">
      <span className={`row-label${muted ? " muted" : ""}`}>
        <label>{label}</label>
        {hint && <Info text={hint} />}
      </span>
      <div className="row-controls">{children}</div>
    </div>
  );
}

function Toggle({ label, hint, checked, onChange }: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  // Tied to the checkbox by id: without it the label would belong to the
  // first control inside it, which is the ⓘ button when there is a hint.
  const id = useId();
  return (
    <label className="toggle" htmlFor={id}>
      <span className="row-label">
        {label}
        {hint && <Info text={hint} />}
      </span>
      <input id={id} type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
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
