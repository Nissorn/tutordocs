// Small DOM helpers shared by the library tools. The reader keeps its own
// private copies; these are for the modules added beside it.

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
