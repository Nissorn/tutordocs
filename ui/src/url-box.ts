import { el, message } from "./dom";
import { ingestUrl } from "./sources-api";

// One box to paste a web page into the library. Enter submits. The fetch is an
// agent job that can run for minutes, so the box shows it is working and the
// rest of the reader stays usable. The reason for a failure is shown as Rust
// worded it.

export function mountUrlBox(host: HTMLElement): void {
  const form = el("form", "url-box");
  form.noValidate = true;
  const input = el("input", "url-input");
  input.type = "text";
  input.placeholder = "Paste a URL to add a source";
  input.setAttribute("aria-label", "Add a source from a web page URL");
  input.inputMode = "url";
  input.autocomplete = "off";
  input.spellcheck = false;
  const note = el("div", "tool-note");
  note.setAttribute("aria-live", "polite");
  note.hidden = true;
  form.append(input, note);
  host.append(form);

  function say(text: string, alert = false): void {
    note.textContent = text;
    note.title = text;
    note.hidden = text === "";
    note.classList.toggle("error", alert);
    if (alert) note.setAttribute("role", "alert");
    else note.removeAttribute("role");
  }

  let fetching = false;

  async function add(): Promise<void> {
    const url = input.value.trim();
    if (url === "" || fetching) return;
    fetching = true;
    input.disabled = true;
    form.setAttribute("aria-busy", "true");
    say("Fetching the page. This can take a minute.");
    try {
      const source = await ingestUrl(url);
      input.value = "";
      say(`Added ${source.name}`);
    } catch (error) {
      say(message(error), true);
    } finally {
      fetching = false;
      input.disabled = false;
      form.removeAttribute("aria-busy");
      // Disabling the box dropped its focus; give it back unless the user moved on.
      if (document.activeElement === null || document.activeElement === document.body) input.focus();
    }
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void add();
  });
}
