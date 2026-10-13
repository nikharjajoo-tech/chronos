import { useEffect, useState } from "react";
import { api } from "./api";
import type { Settings, Snapshot, Theme } from "./types";

export function useSnapshot(): Snapshot | null {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    const off = api.onSnapshot(setSnap);
    api.snapshot().then(setSnap);
    // A hidden window may have missed nothing (events queue), but re-sync on
    // return anyway: it's one cheap call and guards against any lost event.
    const onVis = () => document.visibilityState === "visible" && api.snapshot().then(setSnap);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      off();
      document.removeEventListener("visibilitychange", onVis);
    };
  }, []);
  return snap;
}

export function useSettings(): [Settings | null, (s: Settings) => void] {
  const [settings, setSettings] = useState<Settings | null>(null);
  useEffect(() => {
    const off = api.onSettings(setSettings);
    api.getSettings().then(setSettings);
    return off;
  }, []);
  return [settings, setSettings];
}

const THEME_KEY = "chronos.theme";

/** The theme last seen, so a window paints in it before settings arrive. */
export function cachedTheme(): Theme {
  try {
    return localStorage.getItem(THEME_KEY) === "retro" ? "retro" : "glass";
  } catch {
    return "glass";
  }
}

/** Puts the panel theme on <html data-theme> and remembers it. */
export function useTheme(settings: Settings | null) {
  const theme = settings?.theme;
  useEffect(() => {
    if (!theme) return;
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem(THEME_KEY, theme);
    } catch {
      // Private storage off: the next window just starts in Glass.
    }
  }, [theme]);
}

/** Remaining ms, derived from the absolute end time, never counted down locally. */
export function remainingOf(snap: Snapshot, now = Date.now()): number {
  return snap.run === "running" && snap.endAt !== null ? Math.max(0, snap.endAt - now) : snap.remainingMs;
}

/**
 * Re-render once per displayed second while running, timed to land just after
 * each second boundary so the digits never skip or stall. Stops entirely when
 * the window is hidden; nothing runs in the background.
 */
export function useSecondTick(snap: Snapshot | null): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!snap || snap.run !== "running" || snap.endAt === null) return;
    const endAt = snap.endAt;
    let timer = 0;
    const schedule = () => {
      const t = Date.now();
      setNow(t);
      if (document.visibilityState !== "visible") return;
      const left = endAt - t;
      if (left <= 0) return;
      const delay = (left % 1000 || 1000) + 15;
      timer = window.setTimeout(schedule, delay);
    };
    const onVis = () => {
      clearTimeout(timer);
      schedule();
    };
    schedule();
    document.addEventListener("visibilitychange", onVis);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [snap?.run, snap?.endAt]);
  return now;
}
