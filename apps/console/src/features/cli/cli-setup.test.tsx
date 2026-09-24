import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CliSetupPanel } from "./cli-setup-page";
import { buildChatGptLaunchUrl, buildCliSetupPrompt } from "./cli-setup";

const target = {
  endpoint: "https://cloud.example.test/some/path",
  organizationId: "0198a117-0d8c-70e2-a457-a83c253b9f21",
};

describe("CLI setup launcher", () => {
  it("現在のoriginと組織だけを含むCLI導入プロンプトを生成する", () => {
    const prompt = buildCliSetupPrompt(target);

    expect(prompt).toContain("HeteroCloud CLI を導入し、初期設定まで完了");
    expect(prompt).toContain("https://cloud.example.test");
    expect(prompt).not.toContain("/some/path");
    expect(prompt).toContain(target.organizationId);
    expect(prompt).toContain("Linux/macOS/Windows の x64/ARM64");
    expect(prompt).toContain("SHA-256");
    expect(prompt).toContain("heterocloud --version");
    expect(prompt).toContain("heterocloud auth login");
    expect(prompt).toContain("heterocloud auth status");
    expect(prompt).toContain("APIキーを要求・生成・表示しない");
    expect(prompt).not.toContain("heterocloud.mizuame.app");
  });

  it("Claude CodeとCodexへ同じ検証済みskillを導入するよう指示する", () => {
    const prompt = buildCliSetupPrompt(target);
    expect(prompt).toContain(
      "https://raw.githubusercontent.com/IPA-CyberLab/IPA-RS-HeteroCloud/",
    );
    expect(prompt).toContain(".agents/skills/heterocloud-cli-setup/SKILL.md");
    expect(prompt).toContain(
      "21df216663c8b55b6bd452a8efa5089c6e820625cfe7fbf08d43ee70a69e8091",
    );
    expect(prompt).toContain("~/.claude/skills/heterocloud-cli-setup/SKILL.md");
    expect(prompt).toContain("CODEX_HOME");
    expect(prompt).toContain("~/.codex/skills/heterocloud-cli-setup/SKILL.md");
    expect(prompt).toContain("バックアップ");
  });

  it("公式デスクトップ起動形式へプロンプトをURLエンコードする", () => {
    const prompt = buildCliSetupPrompt(target);
    const launchUrl = buildChatGptLaunchUrl(prompt);
    const parsed = new URL(launchUrl);

    expect(parsed.protocol).toBe("codex:");
    expect(parsed.hostname).toBe("threads");
    expect(parsed.pathname).toBe("/new");
    expect(parsed.searchParams.get("prompt")).toBe(prompt);
  });

  it("ChatGPT起動リンクとコピーフォールバックを表示する", async () => {
    const user = userEvent.setup();
    const writeText = vi.spyOn(navigator.clipboard, "writeText");
    render(<CliSetupPanel {...target} />);

    const launcher = screen.getByRole("link", { name: "ChatGPTでセットアップ" });
    expect(launcher).toHaveAttribute(
      "href",
      expect.stringMatching(/^codex:\/\/threads\/new\?prompt=/),
    );
    expect(screen.getByText(/Claude Code\/Codexのskill導入/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "プロンプトをコピー" }));
    expect(writeText).toHaveBeenCalledWith(buildCliSetupPrompt(target));
    expect(screen.getByText("セットアップ用プロンプトをコピーしました。")).toBeInTheDocument();
  });

  it("HTTP環境だけ明示的な許可設定を追加する", () => {
    const prompt = buildCliSetupPrompt({
      ...target,
      endpoint: "http://lab.example.test:8080",
    });
    expect(prompt).toContain("HETEROCLOUD_ALLOW_INSECURE_HTTP=1");
  });
});
