// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import contract from "./fixtures/sources-contract.json";
import { mountUrlBox } from "./url-box";

type Handler = (args: Record<string, unknown>) => unknown;

/** Rust's `ingest_url`, with every call recorded. */
function backend(ingest: Handler = () => contract.ingest_url) {
  const urls: string[] = [];
  mockIPC((cmd, raw) => {
    if (cmd !== "ingest_url") throw new Error(`unexpected command ${cmd}`);
    const args = (raw ?? {}) as Record<string, unknown>;
    urls.push(args.url as string);
    return ingest(args);
  });
  return urls;
}

let host: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<div id="host"></div>';
  host = document.querySelector<HTMLElement>("#host")!;
  mountUrlBox(host);
});

afterEach(() => {
  clearMocks();
});

function input(): HTMLInputElement {
  return host.querySelector<HTMLInputElement>("input")!;
}

function note(): HTMLElement {
  return host.querySelector<HTMLElement>(".tool-note")!;
}

/** Pastes `text` and presses Enter, which submits the form. */
function submit(text: string): void {
  input().value = text;
  host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

describe("pasting a URL", () => {
  it("asks Rust to fetch it, then says which source was added and clears the box", async () => {
    const urls = backend();
    submit("https://example.com/a");
    await vi.waitFor(() => expect(note().textContent).toBe("Added example-com-a.md"));
    expect(urls).toEqual(["https://example.com/a"]);
    expect(input().value).toBe("");
    expect(note().getAttribute("role")).toBeNull();
  });

  it("trims what was pasted", async () => {
    const urls = backend();
    submit("  https://example.com/a \n");
    await vi.waitFor(() => expect(urls).toEqual(["https://example.com/a"]));
  });

  it("does nothing for an empty box", async () => {
    const urls = backend();
    submit("   ");
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(urls).toEqual([]);
    expect(note().hidden).toBe(true);
  });

  it("is disabled and says it is fetching while the agent works, and starts one job only", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => (release = resolve));
    const urls = backend(async () => {
      await gate;
      return contract.ingest_url;
    });
    submit("https://example.com/a");
    expect(input().disabled).toBe(true);
    expect(note().textContent).toContain("Fetching");
    expect(host.querySelector("form")!.getAttribute("aria-busy")).toBe("true");
    submit("https://example.com/b");
    release();
    await vi.waitFor(() => expect(input().disabled).toBe(false));
    expect(urls).toEqual(["https://example.com/a"]);
  });
});

describe("when the page cannot be added", () => {
  it("shows Rust's reason as an alert and keeps the URL so it can be fixed", async () => {
    backend(() => {
      throw new Error("the agent could not fetch the page: Could not fetch the page: HTTP 404 Not Found");
    });
    submit("https://example.com/gone");
    await vi.waitFor(() => expect(note().getAttribute("role")).toBe("alert"));
    expect(note().textContent).toBe(
      "the agent could not fetch the page: Could not fetch the page: HTTP 404 Not Found",
    );
    expect(input().value).toBe("https://example.com/gone");
    expect(input().disabled).toBe(false);
  });

  it("reads a plain string rejection as the reason too", async () => {
    backend(() => {
      throw "No agent CLI is available. Install Claude Code, Qoder or Antigravity (agy), then return to this window.";
    });
    submit("https://example.com/a");
    await vi.waitFor(() => expect(note().getAttribute("role")).toBe("alert"));
    expect(note().textContent).toContain("No agent CLI is available");
  });

  it("clears the old reason on the next attempt", async () => {
    let attempts = 0;
    backend(() => {
      attempts += 1;
      if (attempts === 1) throw new Error("cannot ingest: not an http or https address");
      return contract.ingest_url;
    });
    submit("example.com");
    await vi.waitFor(() => expect(note().getAttribute("role")).toBe("alert"));
    submit("https://example.com/a");
    await vi.waitFor(() => expect(note().textContent).toBe("Added example-com-a.md"));
    expect(note().getAttribute("role")).toBeNull();
    expect(note().classList.contains("error")).toBe(false);
  });

  it("shows a malformed response as a failure instead of claiming success", async () => {
    backend(() => ({ url: "https://example.com/a" }));
    submit("https://example.com/a");
    await vi.waitFor(() => expect(note().getAttribute("role")).toBe("alert"));
    expect(note().textContent).not.toContain("Added");
  });
});
