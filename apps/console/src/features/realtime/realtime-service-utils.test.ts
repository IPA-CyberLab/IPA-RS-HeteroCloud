import { describe, expect, it } from "vitest";
import {
  formatBytes,
  normalizeEndpoints,
  transferRateSamplesPerHour,
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

  it("累積通信量をリセット対応の1時間換算レートへ変換する", () => {
    expect(
      transferRateSamplesPerHour([
        {
          sampled_at: "2026-08-01T09:15:00Z",
          active_rooms: 2,
          concurrent_connections: 4,
          ingress_bytes: 200_000,
          egress_bytes: 350_000,
          transferred_bytes: 550_000,
        },
        {
          sampled_at: "2026-08-01T09:00:00Z",
          active_rooms: 1,
          concurrent_connections: 2,
          ingress_bytes: 100_000,
          egress_bytes: 150_000,
          transferred_bytes: 250_000,
        },
        {
          sampled_at: "2026-08-01T09:30:00Z",
          active_rooms: 0,
          concurrent_connections: 0,
          ingress_bytes: 25_000,
          egress_bytes: 50_000,
          transferred_bytes: 75_000,
        },
      ]),
    ).toEqual([
      expect.objectContaining({
        sampled_at: "2026-08-01T09:00:00Z",
        ingress_bytes: 0,
        egress_bytes: 0,
        transferred_bytes: 0,
      }),
      expect.objectContaining({
        sampled_at: "2026-08-01T09:15:00Z",
        ingress_bytes: 400_000,
        egress_bytes: 800_000,
        transferred_bytes: 1_200_000,
      }),
      expect.objectContaining({
        sampled_at: "2026-08-01T09:30:00Z",
        ingress_bytes: 100_000,
        egress_bytes: 200_000,
        transferred_bytes: 300_000,
      }),
    ]);
  });
});
