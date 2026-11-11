// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { NOTE_MAX, REWRITE_PRESETS, rewriteForm, rewriteInstruction } from "./rewrite-request";

const SIMPLER =
  "Rewrite for a reader who found this chapter hard. Use plainer words, shorter sentences and one concrete example per idea, and define each term the first time it appears.";

describe("rewriteInstruction", () => {
  it("is undefined for an empty request, so the rewrite stays plain", () => {
    expect(rewriteInstruction({ preset: null, note: "" })).toBeUndefined();
    expect(rewriteInstruction({ preset: null, note: "  \n\t " })).toBeUndefined();
  });

  it("turns each chip into its fixed sentence", () => {
    const sentences = REWRITE_PRESETS.map((p) => rewriteInstruction({ preset: p.id, note: "" }));
    expect(sentences).toEqual([
      SIMPLER,
      "Keep the explanations and add worked examples.",
      "Keep every key idea and cut the rest.",
      "Add the reasoning and edge cases a strong student would ask about.",
    ]);
  });

  it("sends a note alone, trimmed", () => {
    expect(rewriteInstruction({ preset: null, note: "  Use a cooking analogy.\n" })).toBe("Use a cooking analogy.");
  });

  it("puts the chip's sentence first and the note after it", () => {
    expect(rewriteInstruction({ preset: "simpler", note: "I know some biology." })).toBe(
      `${SIMPLER}\n\nI know some biology.`,
    );
  });

  it("cuts a note at the limit", () => {
    const sent = rewriteInstruction({ preset: null, note: "q".repeat(NOTE_MAX + 50) });
    expect(sent).toBe("q".repeat(NOTE_MAX));
  });

  it("names the four chips the learner sees", () => {
    expect(REWRITE_PRESETS.map((p) => p.label)).toEqual([
      "Explain it more simply",
      "Add more examples",
      "Make it shorter",
      "Go deeper",
    ]);
  });
});

describe("rewriteForm", () => {
  const chip = (form: ReturnType<typeof rewriteForm>, preset: string) =>
    form.node.querySelector<HTMLButtonElement>(`button[data-preset="${preset}"]`)!;
  const noteBox = (form: ReturnType<typeof rewriteForm>) => form.node.querySelector<HTMLTextAreaElement>("textarea")!;

  it("starts empty", () => {
    const form = rewriteForm();
    expect(form.request()).toEqual({ preset: null, note: "" });
    expect(form.node.querySelectorAll("button.rewrite-chip")).toHaveLength(4);
    expect(form.node.textContent).toContain("Tell the AI what to change (optional)");
  });

  it("keeps one chip at a time and clears it on a second click", () => {
    const form = rewriteForm();
    chip(form, "simpler").click();
    expect(form.request().preset).toBe("simpler");
    chip(form, "shorter").click();
    expect(form.request().preset).toBe("shorter");
    expect(chip(form, "simpler").getAttribute("aria-pressed")).toBe("false");
    expect(chip(form, "shorter").getAttribute("aria-pressed")).toBe("true");
    chip(form, "shorter").click();
    expect(form.request().preset).toBeNull();
    expect(chip(form, "shorter").getAttribute("aria-pressed")).toBe("false");
  });

  it("reads the note and caps the box at the note limit", () => {
    const form = rewriteForm();
    noteBox(form).value = "Slow down at the proof.";
    expect(form.request().note).toBe("Slow down at the proof.");
    expect(noteBox(form).maxLength).toBe(NOTE_MAX);
  });

  it("disables the chips and the note while a job runs", () => {
    const form = rewriteForm();
    form.setDisabled(true);
    expect(chip(form, "deeper").disabled).toBe(true);
    expect(noteBox(form).disabled).toBe(true);
    form.setDisabled(false);
    expect(chip(form, "deeper").disabled).toBe(false);
    expect(noteBox(form).disabled).toBe(false);
  });
});
