import { describe, expect, it } from "vitest";
import { navigationItems } from "./navigation";

describe("navigationItems", () => {
  it("ownerコンソールではサービス全体の管理項目だけを表示する", () => {
    expect(navigationItems(true)).toEqual([
      { type: "link", text: "全アカウント管理", href: "/overview" },
    ]);
  });

  it("通常コンソールではテナント向けサービスを表示する", () => {
    expect(JSON.stringify(navigationItems(false))).toContain("Flow");
    expect(JSON.stringify(navigationItems(false))).not.toContain("全アカウント管理");
  });
});
