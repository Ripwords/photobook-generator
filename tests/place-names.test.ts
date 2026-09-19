import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

/**
 * `place_names` is the one command that sends anything derived from a photo
 * off the Mac: chapter centres go to Apple's geocoder. The switch is the
 * user's consent, so these tests hold the composable to asking only while it
 * is on.
 */
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { usePlaceNames } = await import("../app/composables/usePlaceNames");

async function settle() {
  for (let i = 0; i < 5; i += 1) await nextTick();
}

const placeNameCalls = () => invoke.mock.calls.filter(([command]) => command === "place_names");

describe("usePlaceNames", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue({ 0: "Kyoto", 2: "Osaka" });
  });

  it("never asks for names while the switch is off, whatever run is shown", async () => {
    const runId = ref(3);
    const places = ref(false);
    const names = usePlaceNames(runId, places);
    await settle();
    runId.value = 4;
    await settle();
    runId.value = 5;
    await settle();

    expect(placeNameCalls()).toEqual([]);
    expect(names.value).toEqual({});
  });

  it("asks for the shown run's names once the switch is on", async () => {
    const runId = ref(3);
    const places = ref(false);
    const names = usePlaceNames(runId, places);
    await settle();
    places.value = true;
    await settle();

    expect(placeNameCalls()).toEqual([["place_names", { runId: 3 }]]);
    expect(names.value).toEqual({ 0: "Kyoto", 2: "Osaka" });
  });

  it("drops the names when the switch goes off, so time chapters are never titled with a town", async () => {
    const runId = ref(3);
    const places = ref(true);
    const names = usePlaceNames(runId, places);
    await settle();
    expect(names.value).toEqual({ 0: "Kyoto", 2: "Osaka" });

    places.value = false;
    await settle();
    expect(names.value).toEqual({});
    runId.value = 4;
    await settle();
    expect(placeNameCalls()).toEqual([["place_names", { runId: 3 }]]);
  });

  it("ignores an answer for a run that is no longer shown", async () => {
    let answerRun3: ((names: Record<number, string>) => void) | undefined;
    invoke.mockImplementation((_command: string, { runId }: { runId: number }) =>
      runId === 3
        ? new Promise((resolve) => {
            answerRun3 = resolve;
          })
        : Promise.resolve({ 1: "Reykjavík" }),
    );
    const runId = ref(3);
    const names = usePlaceNames(runId, ref(true));
    await settle();
    runId.value = 4;
    await settle();
    expect(answerRun3).toBeDefined();
    answerRun3?.({ 0: "Kyoto" });
    await settle();

    expect(names.value).toEqual({ 1: "Reykjavík" });
  });

  it("leaves chapters unnamed when the lookup fails", async () => {
    invoke.mockRejectedValue("the analysed photo set is gone");
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const names = usePlaceNames(ref(3), ref(true));
    await settle();

    expect(names.value).toEqual({});
  });
});
