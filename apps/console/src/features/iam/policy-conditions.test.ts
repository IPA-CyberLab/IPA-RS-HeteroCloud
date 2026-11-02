import { describe, expect, it } from "vitest";
import { policyTimeConditions } from "./policy-conditions";

describe("policy time conditions", () => {
  it("limits a statement to the specified interval", () => {
    expect(policyTimeConditions("2026-11-01T00:00:00Z", "2026-11-02T00:00:00Z")).toEqual({
      DateGreaterThanEquals: { "aws:CurrentTime": ["2026-11-01T00:00:00.000Z"] },
      DateLessThan: { "aws:CurrentTime": ["2026-11-02T00:00:00.000Z"] },
    });
    expect(policyTimeConditions("", "")).toBeUndefined();
  });

  it("rejects invalid or reversed intervals", () => {
    expect(() => policyTimeConditions("bad", "")).toThrow();
    expect(() => policyTimeConditions("2026-11-02T00:00:00Z", "2026-11-01T00:00:00Z")).toThrow();
  });
});
