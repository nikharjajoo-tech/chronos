// What the quick-add boxes (Plan tab and the ⌃⌥T popup) share.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Bucket, ParsedTask } from "../types";

/** Tab cycles where a new task goes. */
export const DEST_CYCLE: Bucket[] = ["today", "week", "backlog"];
export const nextDest = (b: Bucket): Bucket => DEST_CYCLE[(DEST_CYCLE.indexOf(b) + 1) % DEST_CYCLE.length];

/** The engine's reading of typed text, for the preview chips. */
export function useParsedTask(text: string): ParsedTask | null {
  const [parsed, setParsed] = useState<ParsedTask | null>(null);
  const seq = useRef(0);
  useEffect(() => {
    const n = ++seq.current;
    if (!text.trim()) return setParsed(null);
    api.taskParse(text).then((p) => n === seq.current && setParsed(p));
  }, [text]);
  return parsed;
}

/** Briefly true, for the "that can't be added" shake. */
export function useShake(): [boolean, () => void] {
  const [bad, setBad] = useState(false);
  return [bad, () => {
    setBad(true);
    setTimeout(() => setBad(false), 400);
  }];
}
