import "./visuals.css";
import { invoke } from "@tauri-apps/api/core";
import { record, str } from "../api";
import { VISUAL_SANDBOX_ATTRIBUTE } from "./sandbox";
import { PROTOCOL_VERSION, READY_TIMEOUT_MS, start, step, type Effect, type HostEvent } from "./protocol";

type VisualStatus =
  | { status: "accepted"; title: string; estimatedHeight: number; data: unknown }
  | { status: "rejected"; reason: string };

function visualStatus(raw: unknown): VisualStatus {
  const r = record(raw, "describe_visual");
  if (r.status === "rejected") return { status: "rejected", reason: str(r, "reason") };
  if (r.status !== "accepted" || typeof r.estimatedHeight !== "number") {
    throw new Error("describe_visual: unexpected response");
  }
  return { status: "accepted", title: str(r, "title"), estimatedHeight: r.estimatedHeight, data: r.data };
}

/** Returns the current visual theme: forced by data-theme on html or macOS prefers-color-scheme. */
export function currentTheme(): "light" | "dark" {
  const forced = document.documentElement?.dataset?.theme;
  if (forced === "dark" || forced === "light") return forced;
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** One listener for every artifact, keyed by the frame's window so a message is trusted only by its source. */
const sessions = new WeakMap<object, (data: unknown) => void>();
let listening = false;

function listen(): void {
  if (listening) return;
  listening = true;
  window.addEventListener("message", (event) => {
    if (event.source) sessions.get(event.source)?.(event.data);
  });
}

/** Active live frames receiving theme updates. */
const activeFrames = new Set<HTMLIFrameElement>();

export function broadcastTheme(): void {
  const theme = currentTheme();
  for (const frame of activeFrames) {
    if (frame.isConnected && frame.contentWindow) {
      frame.contentWindow.postMessage({ tutordoc: PROTOCOL_VERSION, type: "theme", theme }, "*");
    } else {
      activeFrames.delete(frame);
    }
  }
}

let listeningTheme = false;
function listenTheme(): void {
  if (listeningTheme) return;
  listeningTheme = true;
  window.matchMedia?.("(prefers-color-scheme: dark)")?.addEventListener?.("change", broadcastTheme);
  if (typeof MutationObserver !== "undefined" && document.documentElement) {
    const observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.type === "attributes" && m.attributeName === "data-theme") {
          broadcastTheme();
          break;
        }
      }
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  }
}

/** The visual: scheme maps these two ids to the artifact; it never takes a file path. */
function visualUrl(chapterId: string, visualId: string): string {
  return `visual://localhost/${encodeURIComponent(chapterId)}/${encodeURIComponent(visualId)}/`;
}

/**
 * Mounts every `figure.visual` in a rendered chapter. Each artifact runs in an
 * `allow-scripts` iframe with an opaque origin; if it is refused, reports an
 * error, or does not report ready within 3 seconds, its figure shows the text
 * version with the reason instead. Resolves once every frame is in place.
 */
export async function mountVisuals(prose: HTMLElement, chapterId: string): Promise<void> {
  listen();
  listenTheme();
  const figures = prose.querySelectorAll<HTMLElement>("figure.visual[data-visual-id]");
  await Promise.all([...figures].map((figure) => mountVisual(figure, chapterId)));
}

async function mountVisual(figure: HTMLElement, chapterId: string): Promise<void> {
  figure.querySelectorAll(".visual-frame, .visual-notice, .visual-placeholder").forEach((el) => el.remove());
  const visualId = figure.dataset.visualId!;
  const text = figure.querySelector<HTMLElement>(".visual-fallback")!;
  const began = performance.now();
  const elapsedMs = () => Math.round(performance.now() - began);
  const report = (event: object) =>
    void invoke("record_visual_event", { chapterId, visualId, event }).catch(() => undefined);

  figure.dataset.state = "starting";
  text.hidden = true;

  const placeholder = document.createElement("div");
  placeholder.className = "visual-placeholder";
  placeholder.setAttribute("role", "status");
  placeholder.setAttribute("aria-live", "polite");
  placeholder.textContent = "Loading visual\u2026";
  figure.prepend(placeholder);

  const showText = (reason: string) => {
    figure.querySelectorAll(".visual-frame, .visual-placeholder, .visual-notice").forEach((el) => el.remove());
    figure.dataset.notice = `The interactive figure could not start because ${reason.replace(/\.$/, "")}. Showing the text version.`;
    figure.dataset.state = "fallback";
    text.hidden = false;

    const notice = document.createElement("div");
    notice.className = "visual-notice";
    notice.setAttribute("role", "status");

    const msg = document.createElement("span");
    msg.textContent = `The interactive figure could not start because ${reason.replace(/\.$/, "")}. Showing the text version.`;
    notice.append(msg);

    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "retry visual-retry";
    retry.textContent = "Try again";
    retry.addEventListener("click", () => {
      void mountVisual(figure, chapterId);
    });
    notice.append(retry);

    figure.prepend(notice);
    report({ type: "host.fallback", reason, elapsedMs: elapsedMs() });
  };

  let status: VisualStatus;
  try {
    status = visualStatus(await invoke<unknown>("describe_visual", { chapterId, visualId }));
  } catch (error) {
    status = { status: "rejected", reason: error instanceof Error ? error.message : String(error) };
  }
  if (!figure.isConnected) return;
  if (status.status === "rejected") return showText(`it was refused: ${status.reason}`);
  const { data, title, estimatedHeight } = status;

  placeholder.style.height = `${estimatedHeight}px`;

  const frame = document.createElement("iframe");
  // The sandbox is set before src so the document never loads without it.
  frame.setAttribute("sandbox", VISUAL_SANDBOX_ATTRIBUTE);
  frame.className = "visual-frame";
  frame.title = title;
  frame.style.height = `${estimatedHeight}px`;
  frame.src = visualUrl(chapterId, visualId);

  let session = start(visualId, estimatedHeight);
  const timer = window.setTimeout(() => dispatch({ kind: "timeout" }), READY_TIMEOUT_MS);

  function apply(effect: Effect): void {
    switch (effect.kind) {
      case "send-init": {
        const theme = currentTheme();
        frame.contentWindow?.postMessage(
          { tutordoc: PROTOCOL_VERSION, type: "init", visualId, chapterId, data, theme },
          "*",
        );
        return;
      }
      case "show":
        placeholder.remove();
        frame.style.height = `${effect.height}px`;
        figure.dataset.state = "live";
        activeFrames.add(frame);
        report({ type: "host.live", height: effect.height, elapsedMs: elapsedMs() });
        return;
      case "resize":
        frame.style.height = `${effect.height}px`;
        return;
      case "forward":
        report(effect.message);
        return;
      case "fallback":
        activeFrames.delete(frame);
        frame.remove();
        showText(effect.reason);
        return;
    }
  }

  function dispatch(event: HostEvent): void {
    const next = step(session, event);
    session = next.session;
    if (session.phase !== "starting") window.clearTimeout(timer);
    next.effects.forEach(apply);
  }

  frame.addEventListener("load", () => dispatch({ kind: "loaded" }));
  figure.prepend(frame);
  if (frame.contentWindow) sessions.set(frame.contentWindow, (message) => dispatch({ kind: "message", data: message }));
}
