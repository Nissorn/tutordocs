// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "../fixtures/visual-contract.json";
import { renderMarkdown } from "../markdown";
import { mountVisuals } from "./host";

const CHAPTER = [
  "# Solids",
  "",
  "```visual cube",
  "A cube has **six** faces.",
  "```",
  "",
  "```visual cdn",
  "A cube is a regular hexahedron.",
  "```",
  "",
].join("\n");

let recorded: Array<{ visualId: string; event: Record<string, unknown> }>;

function backend(): void {
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    if (cmd === "describe_visual") {
      return a.visualId === "cube" ? contract.describe_visual_accepted : contract.describe_visual_rejected;
    }
    if (cmd === "record_visual_event") {
      recorded.push({ visualId: a.visualId as string, event: a.event as Record<string, unknown> });
      return null;
    }
    throw new Error(`unexpected command ${cmd}`);
  });
}

async function mountChapter(): Promise<HTMLElement> {
  const prose = document.createElement("div");
  prose.className = "prose";
  prose.innerHTML = renderMarkdown(CHAPTER);
  document.body.append(prose);
  await mountVisuals(prose, "shapes");
  return prose;
}

const figure = (prose: HTMLElement, id: string) =>
  prose.querySelector<HTMLElement>(`figure.visual[data-visual-id="${id}"]`)!;

function fromArtifact(frame: HTMLIFrameElement, data: unknown): void {
  window.dispatchEvent(new MessageEvent("message", { data, source: frame.contentWindow as Window }));
}

/**
 * happy-dom does not load iframe pages, so a frame has no window of its own.
 * Each frame gets a stand-in window that can receive postMessage, which is all
 * the host touches.
 */
const frameWindows = new WeakMap<HTMLIFrameElement, { postMessage: (message: unknown) => void }>();
Object.defineProperty(HTMLIFrameElement.prototype, "contentWindow", {
  configurable: true,
  get(this: HTMLIFrameElement) {
    if (!this.isConnected) return null;
    if (!frameWindows.has(this)) frameWindows.set(this, { postMessage: () => undefined });
    return frameWindows.get(this);
  },
});

beforeEach(() => {
  recorded = [];
  document.body.innerHTML = "";
  vi.useFakeTimers();
  backend();
});

afterEach(() => {
  vi.useRealTimers();
  clearMocks();
});

describe("visual host", () => {
  it("renders the fenced block as a figure that carries its text version", () => {
    const html = renderMarkdown(CHAPTER);
    expect(html).toContain('<figure class="visual" data-visual-id="cube">');
    expect(html).toContain("A cube has <strong>six</strong> faces.");
    expect(renderMarkdown("```visual ../x\nhi\n```")).toContain("<pre><code");
  });

  it("loads an accepted artifact in a sandboxed iframe from the visual scheme with a starting placeholder", async () => {
    const prose = await mountChapter();
    const frame = figure(prose, "cube").querySelector("iframe")!;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("src")).toBe("visual://localhost/shapes/cube/");
    expect(frame.style.height).toBe("360px");
    expect(figure(prose, "cube").dataset.state).toBe("starting");
    const placeholder = figure(prose, "cube").querySelector(".visual-placeholder")!;
    expect(placeholder).not.toBeNull();
    expect(placeholder.getAttribute("role")).toBe("status");
  });

  it("sends init with the artifact's data on load and shows the frame on ready", async () => {
    const prose = await mountChapter();
    const frame = figure(prose, "cube").querySelector("iframe")!;
    const sent: unknown[] = [];
    vi.spyOn(frame.contentWindow!, "postMessage").mockImplementation((message: unknown) => void sent.push(message));
    frame.dispatchEvent(new Event("load"));
    expect(sent).toEqual([
      expect.objectContaining({
        tutordoc: 1,
        type: "init",
        visualId: "cube",
        chapterId: "shapes",
        data: contract.describe_visual_accepted.data,
      }),
    ]);
    fromArtifact(frame, { tutordoc: 1, type: "ready", visualId: "cube", height: 300 });
    expect(figure(prose, "cube").dataset.state).toBe("live");
    expect(frame.style.height).toBe("300px");
    expect(figure(prose, "cube").querySelector<HTMLElement>(".visual-fallback")!.hidden).toBe(true);
    expect(figure(prose, "cube").querySelector(".visual-placeholder")).toBeNull();
    vi.advanceTimersByTime(5000);
    expect(figure(prose, "cube").dataset.state).toBe("live");
  });

  it("propagates live theme changes to mounted visuals", async () => {
    const prose = await mountChapter();
    const frame = figure(prose, "cube").querySelector("iframe")!;
    const sent: unknown[] = [];
    vi.spyOn(frame.contentWindow!, "postMessage").mockImplementation((message: unknown) => void sent.push(message));
    frame.dispatchEvent(new Event("load"));
    fromArtifact(frame, { tutordoc: 1, type: "ready", visualId: "cube", height: 300 });
    sent.length = 0;

    document.documentElement.dataset.theme = "dark";
    await vi.waitFor(() => expect(sent).toContainEqual({ tutordoc: 1, type: "theme", theme: "dark" }));

    document.documentElement.dataset.theme = "light";
    await vi.waitFor(() => expect(sent).toContainEqual({ tutordoc: 1, type: "theme", theme: "light" }));
    delete document.documentElement.dataset.theme;
  });

  it("falls back to the visible text version with role=status notice and retry when ready does not come within 3 seconds", async () => {
    const prose = await mountChapter();
    const cube = figure(prose, "cube");
    vi.advanceTimersByTime(2999);
    expect(cube.dataset.state).toBe("starting");
    vi.advanceTimersByTime(1);
    expect(cube.dataset.state).toBe("fallback");
    expect(cube.querySelector("iframe")).toBeNull();
    expect(cube.querySelector<HTMLElement>(".visual-fallback")!.hidden).toBe(false);
    expect(cube.dataset.notice).toContain("did not report ready within 3 seconds");
    const notice = cube.querySelector<HTMLElement>(".visual-notice[role='status']")!;
    expect(notice).not.toBeNull();
    expect(notice.textContent).toContain("did not report ready within 3 seconds");
    const retry = notice.querySelector<HTMLButtonElement>("button.retry")!;
    expect(retry).not.toBeNull();
    expect(recorded).toContainEqual({
      visualId: "cube",
      event: expect.objectContaining({ type: "host.fallback" }),
    });

    // Clicking retry starts the figure again
    retry.click();
    expect(cube.dataset.state).toBe("starting");
    expect(cube.querySelector(".visual-placeholder[role='status']")).not.toBeNull();
  });

  it("surfaces an error from the artifact as the fallback without breaking the reader", async () => {
    const prose = await mountChapter();
    const cube = figure(prose, "cube");
    const frame = cube.querySelector("iframe")!;
    fromArtifact(frame, { tutordoc: 1, type: "error", visualId: "cube", message: "TypeError: chart is null" });
    expect(cube.dataset.state).toBe("fallback");
    expect(cube.dataset.notice).toContain("TypeError: chart is null");
    expect(cube.querySelector<HTMLElement>(".visual-notice[role='status']")!.textContent).toContain("TypeError: chart is null");
    expect(cube.querySelector<HTMLElement>(".visual-fallback")!.textContent).toContain("six faces");
    expect(prose.querySelector("h1")!.textContent).toBe("Solids");
    expect(recorded).toContainEqual({
      visualId: "cube",
      event: { type: "error", message: "TypeError: chart is null" },
    });
  });

  it("ignores messages from anything but the artifact's own frame", async () => {
    const prose = await mountChapter();
    window.dispatchEvent(
      new MessageEvent("message", { data: { tutordoc: 1, type: "error", message: "forged" }, source: window }),
    );
    expect(figure(prose, "cube").dataset.state).toBe("starting");
  });

  it("shows a rejected artifact's text version at once with the reason", async () => {
    const prose = await mountChapter();
    const cdn = figure(prose, "cdn");
    expect(cdn.dataset.state).toBe("fallback");
    expect(cdn.querySelector("iframe")).toBeNull();
    expect(cdn.dataset.notice).toContain("https://cdn.jsdelivr.net/");
    expect(cdn.querySelector<HTMLElement>(".visual-fallback")!.textContent).toContain("regular hexahedron");
  });
});
