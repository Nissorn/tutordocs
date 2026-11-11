import { vi } from "vitest";

export interface MatchMediaController {
  setMatches(matches: boolean): void;
  restore(): void;
  readonly listenerCount: number;
}

export function stubMatchMedia(initialMatches = false): MatchMediaController {
  const original = window.matchMedia;
  let matches = initialMatches;
  const listeners = new Set<(e: { matches: boolean }) => void>();

  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    get matches() {
      return matches;
    },
    media: query,
    onchange: null,
    addListener: (cb: (e: { matches: boolean }) => void) => listeners.add(cb),
    removeListener: (cb: (e: { matches: boolean }) => void) => listeners.delete(cb),
    addEventListener: (_event: string, cb: (e: { matches: boolean }) => void) => listeners.add(cb),
    removeEventListener: (_event: string, cb: (e: { matches: boolean }) => void) => listeners.delete(cb),
    dispatchEvent: vi.fn(),
  }));

  return {
    setMatches(m: boolean) {
      matches = m;
      for (const cb of [...listeners]) {
        cb({ matches: m });
      }
    },
    restore() {
      window.matchMedia = original;
    },
    get listenerCount() {
      return listeners.size;
    },
  };
}

export function stubWideMatchMedia(): () => void {
  const ctl = stubMatchMedia(false);
  return () => ctl.restore();
}
