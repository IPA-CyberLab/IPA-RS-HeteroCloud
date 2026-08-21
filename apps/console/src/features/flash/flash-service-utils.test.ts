import { describe, expect, it } from "vitest";
import type { FlashServiceStatus } from "@/lib/api-types";
import { flashServiceEndpoints } from "./flash-service-utils";

describe("flashServiceEndpoints", () => {
  it("構造化されたTCP/UDPエンドポイントを表示形式へ変換する", () => {
    const status: FlashServiceStatus = {
      endpoints: [
        { name: "game", protocol: "udp", host: "203.0.113.10", port: 7777 },
        { name: "admin", protocol: "TCP", url: "https://flash.example.com" },
      ],
    };

    expect(flashServiceEndpoints(status)).toEqual([
      expect.objectContaining({
        name: "game",
        protocol: "UDP",
        address: "203.0.113.10:7777",
      }),
      expect.objectContaining({
        name: "admin",
        protocol: "TCP",
        address: "https://flash.example.com",
      }),
    ]);
  });

  it("名前をキーにしたendpoint mapも受け付ける", () => {
    expect(
      flashServiceEndpoints({
        endpoints: {
          udp: "udp://203.0.113.10:7777",
          tcp: ["tcp://203.0.113.11:8080"],
        },
      }),
    ).toEqual([
      expect.objectContaining({ name: "udp", address: "udp://203.0.113.10:7777" }),
      expect.objectContaining({ name: "tcp", address: "tcp://203.0.113.11:8080" }),
    ]);
  });

  it("Outboxが保存したprovider statusを展開する", () => {
    expect(
      flashServiceEndpoints({
        operation_id: "operation-1",
        status: {
          ready_replicas: 2,
          endpoints: [
            {
              name: "udp",
              protocol: "udp",
              host: "203.0.113.20",
              port: 7777,
            },
          ],
        },
      }),
    ).toEqual([
      expect.objectContaining({
        name: "udp",
        protocol: "UDP",
        address: "203.0.113.20:7777",
      }),
    ]);
  });
});
