import { describe, expect, it } from "vitest";
import { VISUAL_SANDBOX_ATTRIBUTE } from "./sandbox";

describe("visual iframe sandbox", () => {
  it("is exactly allow-scripts", () => {
    expect(VISUAL_SANDBOX_ATTRIBUTE).toBe("allow-scripts");
  });

  it("never grants same-origin access", () => {
    expect(VISUAL_SANDBOX_ATTRIBUTE.split(/\s+/)).not.toContain("allow-same-origin");
  });
});