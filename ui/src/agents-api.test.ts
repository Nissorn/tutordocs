// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { driverTuning, listDriverModels, setDriverEffort, setDriverModel } from "./agents-api";

afterEach(() => {
  clearMocks();
});

describe("model and effort commands", () => {
  it("lists models and keeps a null id for the CLI default", async () => {
    let seen: unknown;
    mockIPC((cmd, args) => {
      expect(cmd).toBe("list_driver_models");
      seen = args;
      return {
        models: [
          { id: null, label: "CLI default", efforts: [] },
          { id: "opus", label: "opus", efforts: ["low", "high"] },
        ],
        efforts: ["low", "high"],
        accepts_custom_model: true,
      };
    });

    await expect(listDriverModels("claude")).resolves.toEqual({
      models: [
        { id: null, label: "CLI default", efforts: [] },
        { id: "opus", label: "opus", efforts: ["low", "high"] },
      ],
      efforts: ["low", "high"],
      acceptsCustomModel: true,
    });
    expect(seen).toEqual({ id: "claude" });
  });

  it("reads and sets the model and effort for one driver", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "driver_tuning") return { model: null, effort: null };
      if (cmd === "set_driver_model") return { model: "opus", effort: null };
      if (cmd === "set_driver_effort") return { model: "opus", effort: "xhigh" };
      throw new Error(`unexpected command ${cmd}`);
    });

    await expect(driverTuning("agy")).resolves.toEqual({ model: null, effort: null });
    await expect(setDriverModel("agy", "opus")).resolves.toEqual({ model: "opus", effort: null });
    await expect(setDriverEffort("agy", "xhigh")).resolves.toEqual({ model: "opus", effort: "xhigh" });
    await expect(setDriverModel("agy", null)).resolves.toEqual({ model: "opus", effort: null });
    await expect(setDriverEffort("agy", null)).resolves.toEqual({ model: "opus", effort: "xhigh" });

    expect(calls.map((call) => call.cmd)).toEqual([
      "driver_tuning",
      "set_driver_model",
      "set_driver_effort",
      "set_driver_model",
      "set_driver_effort",
    ]);
    expect(calls[1].args).toEqual({ id: "agy", model: "opus" });
    expect(calls[2].args).toEqual({ id: "agy", effort: "xhigh" });
    expect(calls[3].args).toEqual({ id: "agy", model: null });
    expect(calls[4].args).toEqual({ id: "agy", effort: null });
  });

  it("keeps a level outside the old five when it is a lowercase word", async () => {
    mockIPC(() => ({ model: "opus", effort: "turbo" }));
    await expect(driverTuning("claude")).resolves.toEqual({ model: "opus", effort: "turbo" });
  });

  it("rejects an effort that is not lowercase letters", async () => {
    mockIPC(() => ({ model: "opus", effort: "-low" }));
    await expect(driverTuning("claude")).rejects.toThrow(/effort/);
  });
});
