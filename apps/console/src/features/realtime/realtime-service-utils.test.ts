import { describe, expect, it } from "vitest";
import {
  formatBytes,
  normalizeEndpoints,
  transferredBytes,
} from "./realtime-service-utils";

describe("realtime service utilities", () => {
  it("確定APIのエンドポイント群を保持する", () => {
    expect(
      normalizeEndpoints({
        api: ["https://api.example.com"],
        signaling: ["wss://signal.example.com"],
        livekit: ["wss://livekit.example.com"],
        stun: ["stun:turn.example.com:3478"],
        turn: ["turns:turn.example.com:5349"],
      }),
    ).toEqual({
      api: ["https://api.example.com"],
      signaling: ["wss://signal.example.com"],
      livekit: ["wss://livekit.example.com"],
      stun: ["stun:turn.example.com:3478"],
      turn: ["turns:turn.example.com:5349"],
    });
  });

  it("認証情報のFlow API配列をAPIエンドポイントとして扱う", () => {
    expect(normalizeEndpoints(["https://flow-a.example.com"]).api).toEqual([
      "https://flow-a.example.com",
    ]);
  });

  it("通信量を可読化し、合計値がなければingressとegressから算出する", () => {
    expect(formatBytes(1_250_000)).toBe("1.25 MB");
    expect(
      transferredBytes({
        ingress_bytes: 1_000,
        egress_bytes: 2_000,
        transferred_bytes: Number.NaN,
      }),
    ).toBe(3_000);
  });
});
