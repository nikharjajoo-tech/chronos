export const MINUTE = 60_000;

/** Countdown display. Rounds up so "0:00" only shows when time is truly up. */
export function fmtClock(ms: number): string {
  const total = Math.ceil(Math.max(0, ms) / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** "2h 18m", "47m", "0m". */
export function fmtDuration(ms: number): string {
  const mins = Math.round(Math.max(0, ms) / MINUTE);
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h === 0) return `${m}m`;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

/** A face for an average session rating (1–4). */
export const ratingFace = (r: number) => (r < 1.75 ? "😫" : r < 2.5 ? "😐" : r < 3.25 ? "🙂" : "🔥");

export function fmtTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/**
 * Parse what someone types into the clock: "25" → 25 min, "18:42" → 18m42s,
 * "1h", "90m", "1:30:00". Returns ms, or null if it isn't a duration.
 */
export function parseDuration(input: string): number | null {
  const s = input.trim().toLowerCase().replace(/\s+/g, "");
  if (!s) return null;
  let ms: number | null = null;
  let m: RegExpMatchArray | null;
  if ((m = s.match(/^(\d{1,3})$/))) ms = +m[1] * MINUTE;
  else if ((m = s.match(/^(\d{1,3}):([0-5]\d)$/))) ms = +m[1] * MINUTE + +m[2] * 1000;
  else if ((m = s.match(/^(\d{1,2}):([0-5]\d):([0-5]\d)$/))) ms = (+m[1] * 60 + +m[2]) * MINUTE + +m[3] * 1000;
  else if ((m = s.match(/^(?:(\d{1,2})h)?(?:(\d{1,3})m(?:in)?)?$/)) && (m[1] || m[2]))
    ms = (+(m[1] ?? 0) * 60 + +(m[2] ?? 0)) * MINUTE;
  if (ms === null || ms <= 0 || ms > 6 * 60 * MINUTE) return null;
  return ms;
}

/** Keyboard event → global-shortcut accelerator, e.g. "CommandOrControl+Shift+H". */
export function shortcutFromEvent(e: Pick<KeyboardEvent, "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "code">, isMac: boolean): string | null {
  const key = e.code
    .replace(/^Key/, "")
    .replace(/^Digit/, "")
    .replace(/^Arrow/, "");
  if (/^(Meta|Control|Alt|Shift|OS)(Left|Right)?$/.test(key)) return null;
  const mods: string[] = [];
  if (isMac ? e.metaKey : e.ctrlKey) mods.push("CommandOrControl");
  if (isMac && e.ctrlKey) mods.push("Control");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (mods.length === 0) return null; // a bare key would hijack typing everywhere
  return [...mods, key].join("+");
}

/** "CommandOrControl+Shift+H" → "⌘⇧H" */
export function prettyShortcut(s: string): string {
  return s
    .split("+")
    .map((p) => {
      switch (p.toLowerCase()) {
        case "commandorcontrol":
        case "cmdorctrl":
        case "command":
        case "cmd":
        case "super":
          return "⌘";
        case "shift":
          return "⇧";
        case "alt":
        case "option":
          return "⌥";
        case "control":
        case "ctrl":
          return "⌃";
        default:
          return p.toUpperCase();
      }
    })
    .join("");
}
