import { describe, expect, it } from "vitest";
import { fmtClock, fmtDuration, MINUTE, parseDuration, prettyShortcut, shortcutFromEvent } from "./format";

describe("fmtClock", () => {
  it("rounds partial seconds up so 0:00 means done", () => {
    expect(fmtClock(18 * MINUTE + 41_001)).toBe("18:42");
    expect(fmtClock(1)).toBe("00:01");
    expect(fmtClock(0)).toBe("00:00");
    expect(fmtClock(-5)).toBe("00:00");
    expect(fmtClock(120 * MINUTE)).toBe("120:00");
  });
});

describe("fmtDuration", () => {
  it("formats hours and minutes", () => {
    expect(fmtDuration(138 * MINUTE)).toBe("2h 18m");
    expect(fmtDuration(47 * MINUTE)).toBe("47m");
    expect(fmtDuration(180 * MINUTE)).toBe("3h");
    expect(fmtDuration(0)).toBe("0m");
  });
});

describe("parseDuration", () => {
  it.each([
    ["25", 25 * MINUTE],
    [" 45 ", 45 * MINUTE],
    ["18:42", 18 * MINUTE + 42_000],
    ["1:30:00", 90 * MINUTE],
    ["1h", 60 * MINUTE],
    ["90m", 90 * MINUTE],
    ["1h 15m", 75 * MINUTE],
    ["10min", 10 * MINUTE],
  ])("%s", (input, ms) => expect(parseDuration(input)).toBe(ms));

  it.each(["", "abc", "0", "12:75", "-5", "999"])("rejects %s", (input) => expect(parseDuration(input)).toBeNull());
});

describe("shortcuts", () => {
  const ev = (code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
    metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, code, ...mods,
  });
  it("builds accelerators from key events", () => {
    expect(shortcutFromEvent(ev("KeyH", { metaKey: true, shiftKey: true }), true)).toBe("CommandOrControl+Shift+H");
    expect(shortcutFromEvent(ev("Digit9", { ctrlKey: true, altKey: true }), false)).toBe("CommandOrControl+Alt+9");
    expect(shortcutFromEvent(ev("ArrowUp", { metaKey: true }), true)).toBe("CommandOrControl+Up");
  });
  it("ignores bare keys and lone modifiers", () => {
    expect(shortcutFromEvent(ev("KeyH"), true)).toBeNull();
    expect(shortcutFromEvent(ev("ShiftLeft", { shiftKey: true }), true)).toBeNull();
  });
  it("pretty-prints", () => {
    expect(prettyShortcut("CommandOrControl+Shift+H")).toBe("⌘⇧H");
  });
});
