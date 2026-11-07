import {QueryClient, QueryClientProvider} from "@tanstack/react-query";
import {render, screen, waitFor} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {beforeEach, describe, expect, it, vi} from "vitest";
import {api} from "@/lib/api-client";
import {FlashSshPanel} from "./flash-ssh-panel";

vi.mock("@/features/auth/session", () => ({useSession: () => ({data: {user: {id: "user-a"}}})}));

const key = {id:"key-a", organization_id:"org", user_id:"user-a", name:"Laptop", public_key:"ssh-ed25519 AAAA", fingerprint:"SHA256:key-a", github_username:null, created_at:"2026-01-01T00:00:00Z"};
function panel() {
  const client = new QueryClient({defaultOptions:{queries:{retry:false}, mutations:{retry:false}}});
  return render(<QueryClientProvider client={client}><FlashSshPanel organizationId="org" serviceId="service" /></QueryClientProvider>);
}

describe("Flash SSH keys", () => {
  beforeEach(() => {
    vi.spyOn(api.flash.sshKeys, "list").mockResolvedValue({items:[key]});
    vi.spyOn(api.flash.sshKeys, "forService").mockResolvedValue({key_ids:[]});
    vi.spyOn(api.flash.sshKeys, "info").mockResolvedValue({enabled:true, host_key_fingerprint:"SHA256:host", tcp:null});
  });
  it("selects a registered key and grants it only to the displayed service", async () => {
    const set = vi.spyOn(api.flash.sshKeys, "setForService").mockResolvedValue({key_ids:[key.id]});
    const user = userEvent.setup(); panel();
    const select = await screen.findByRole("combobox", {name:"接続を許可する公開鍵"});
    await screen.findByRole("option", {name:/Laptop/});
    await user.selectOptions(select, key.id);
    await user.click(screen.getByRole("button", {name:"この鍵で接続を許可"}));
    expect(set).toHaveBeenCalledWith("org", "service", key.id, true);
    expect(await screen.findByRole("button", {name:"接続許可を解除"})).toBeVisible();
    await user.click(screen.getByRole("button", {name:"接続許可を解除"}));
    expect(set).toHaveBeenLastCalledWith("org", "service", key.id, false);
  });
  it("registers a pasted key and previews GitHub keys before importing", async () => {
    const create = vi.spyOn(api.flash.sshKeys, "create").mockResolvedValue(key);
    const preview = vi.spyOn(api.flash.sshKeys, "previewGithub").mockResolvedValue({username:"alice",items:[{public_key:"ssh-ed25519 GithubKey",fingerprint:"SHA256:github"}]});
    const user = userEvent.setup(); panel();
    await user.type(screen.getByRole("textbox", {name:"鍵の名前"}), "Laptop");
    await user.type(screen.getByRole("textbox", {name:"公開鍵"}), "ssh-ed25519 AAAA");
    await user.click(screen.getByRole("button", {name:"公開鍵を登録"}));
    await waitFor(() => expect(create).toHaveBeenCalledWith("org", {name:"Laptop",public_key:"ssh-ed25519 AAAA"}));
    await user.type(screen.getByRole("textbox", {name:"GitHub ユーザー名"}), "alice");
    await user.click(screen.getByRole("button", {name:"公開鍵を検索"}));
    expect(preview).toHaveBeenCalledWith("org", "alice");
    expect(await screen.findByText("SHA256:github")).toBeVisible();
    await user.click(screen.getByRole("button", {name:"この鍵を登録"}));
    expect(create).toHaveBeenLastCalledWith("org", {name:"alice · 6:github",public_key:"ssh-ed25519 GithubKey",github_username:"alice"});
  });
});
