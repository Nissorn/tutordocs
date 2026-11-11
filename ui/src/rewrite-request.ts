// What the learner asks a rewrite to change: one quick choice, a note, or both.
// The reader turns it into the single `instruction` string Rust takes.

import { el } from "./dom";

export type RewritePreset = "simpler" | "more-examples" | "shorter" | "deeper";

export interface RewriteRequest {
  preset: RewritePreset | null;
  /** What the learner typed. `rewriteInstruction` trims it and cuts it at `NOTE_MAX`. */
  note: string;
}

export const NOTE_MAX = 500;

interface PresetInfo {
  id: RewritePreset;
  /** The chip's text. */
  label: string;
  /** What the agent is told when the chip is chosen. */
  sentence: string;
}

export const REWRITE_PRESETS: readonly PresetInfo[] = [
  {
    id: "simpler",
    label: "Explain it more simply",
    sentence:
      "Rewrite for a reader who found this chapter hard. Use plainer words, shorter sentences and one concrete example per idea, and define each term the first time it appears.",
  },
  {
    id: "more-examples",
    label: "Add more examples",
    sentence: "Keep the explanations and add worked examples.",
  },
  {
    id: "shorter",
    label: "Make it shorter",
    sentence: "Keep every key idea and cut the rest.",
  },
  {
    id: "deeper",
    label: "Go deeper",
    sentence: "Add the reasoning and edge cases a strong student would ask about.",
  },
];

/** The instruction Rust takes, or `undefined` for an empty request, which keeps a plain rewrite. */
export function rewriteInstruction(request: RewriteRequest): string | undefined {
  const sentence = REWRITE_PRESETS.find((p) => p.id === request.preset)?.sentence;
  const note = request.note.trim().slice(0, NOTE_MAX).trim();
  const parts = [sentence, note].filter((part): part is string => !!part);
  return parts.length > 0 ? parts.join("\n\n") : undefined;
}

export interface RewriteForm {
  node: HTMLElement;
  request(): RewriteRequest;
  setDisabled(disabled: boolean): void;
}

/** The four chips (one at a time, a second click clears it) and the note box. */
export function rewriteForm(): RewriteForm {
  let preset: RewritePreset | null = null;

  const chips = el("div", "rewrite-chips");
  chips.setAttribute("role", "group");
  chips.setAttribute("aria-label", "How to rewrite it");
  const buttons = REWRITE_PRESETS.map((info) => {
    const chip = el("button", "quiet-button rewrite-chip", info.label);
    chip.type = "button";
    chip.dataset.preset = info.id;
    chip.setAttribute("aria-pressed", "false");
    chip.addEventListener("click", () => {
      preset = preset === info.id ? null : info.id;
      for (const other of buttons) other.setAttribute("aria-pressed", String(other.dataset.preset === preset));
    });
    return chip;
  });
  chips.append(...buttons);

  const field = el("label", "rewrite-note");
  const caption = el("span", "rewrite-note-label", "Tell the AI what to change (optional)");
  const note = el("textarea", "rewrite-note-input");
  note.rows = 3;
  note.maxLength = NOTE_MAX;
  field.append(caption, note);

  const node = el("div", "rewrite-form");
  node.append(chips, field);

  return {
    node,
    request: () => ({ preset, note: note.value }),
    setDisabled(disabled) {
      for (const chip of buttons) chip.disabled = disabled;
      note.disabled = disabled;
    },
  };
}
