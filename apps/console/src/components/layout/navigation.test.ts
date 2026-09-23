import { describe, expect, it } from "vitest";
import { navigationItems } from "./navigation";

describe("navigationItems", () => {
  it("ownerコンソールではサービス全体の管理項目だけを表示する", () => {
    expect(navigationItems(true)).toEqual([
      { type: "link", text: "全アカウント管理", href: "/overview" },
      { type: "link", text: "コスト管理", href: "/owner/cost-management" },
      { type: "link", text: "GPU管理", href: "/owner/gpus" },
    ]);
  });

  it("通常コンソールではテナント向けサービスを表示する", () => {
    const items = navigationItems(false);
    const flash = items.find(
      (item) => item.type === "section" && item.text === "Flash",
    );

    expect(flash).toEqual({
      type: "section",
      text: "Flash",
      items: [
        { type: "link", text: "サービス", href: "/flash/services" },
        { type: "link", text: "イメージ", href: "/registry" },
        { type: "link", text: "コスト管理", href: "/cost-management" },
      ],
    });
    expect(items).not.toContainEqual(
      expect.objectContaining({ type: "section", text: "Flash Registry" }),
    );
    expect(JSON.stringify(items)).toContain("Flow");
    expect(JSON.stringify(items)).toContain("Syouyu");
    expect(JSON.stringify(items)).toContain("/syouyu/buckets");
    expect(JSON.stringify(items)).toContain("/cli");
    expect(JSON.stringify(items)).toContain("CLIセットアップ");
    expect(JSON.stringify(items)).not.toContain("全アカウント管理");
  });
});
