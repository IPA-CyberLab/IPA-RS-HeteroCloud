import { describe, expect, it } from "vitest";
import type { FlashServiceStatus } from "@/lib/api-types";
import { flashServiceEndpoints, requestedReplicas, readyReplicas, flashScaleLabel } from "./flash-service-utils";
import { defaultFlashServiceFormValue, flashSpecFromForm } from "./flash-service-form";

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
        address: "flash.example.com",
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

it("renders provider LB hostnames and IPv6 with transport ports", () => {
  expect(flashServiceEndpoints({ status: { endpoints: [
    { protocol: "tcp", address: "lb.example.test", port: 443, url: "https://wrong.example.test" },
    { protocol: "udp", address: "lb.example.test", port: 7777 },
    { protocol: "tcp", address: "2001:db8::1", port: 8080 },
  ] } }).map((endpoint) => endpoint.address)).toEqual(["lb.example.test:443", "lb.example.test:7777", "[2001:db8::1]:8080"]);
});

it("separates configured scaling from provider requested replicas", () => {
  const spec = { ...flashSpecFromForm(defaultFlashServiceFormValue), ports: [] };
  expect(requestedReplicas({ spec, status: {} })).toBe(1);
  spec.autoscaling = { min_replicas: 1, max_replicas: 5, target_cpu_utilization_percent: 80 };
  expect(flashScaleLabel(spec)).toBe("自動・1〜5");
  expect(requestedReplicas({ spec, status: {} })).toBeNull();
  expect(requestedReplicas({ spec, status: { status: { desired_replicas: 4, ready_replicas: 3 } } })).toBe(4);
  expect(requestedReplicas({ spec, status: { status: { requested_replicas: 4, ready_replicas: 3 } } })).toBe(4);
  expect(requestedReplicas({ spec, status: { replicas: 2 } })).toBe(2);
  const unavailable = { status: { desired_replicas: 4, ready_replicas: 3, live_status_unavailable: true } };
  expect(requestedReplicas({ spec, status: unavailable })).toBeNull();
  expect(readyReplicas({ status: unavailable })).toBeNull();
});
