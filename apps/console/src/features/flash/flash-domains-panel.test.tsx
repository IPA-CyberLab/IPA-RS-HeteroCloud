import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "@/lib/api-client";
import { FlashDomainsPanel } from "./flash-domains-panel";

const domain = {id:"domain-one",hostname:"app.example.org",phase:"pending_dns" as const,cname_target:"f-service.flash.example.org",verification:{type:"TXT" as const,name:"_heterocloud.app.example.org",value:"heterocloud-domain=proof"},oidc_callback_url:"https://app.example.org/_heterocloud/oidc/callback"};
function panel(editable=true) {
  const client=new QueryClient({defaultOptions:{queries:{retry:false},mutations:{retry:false}}});
  return render(<QueryClientProvider client={client}><FlashDomainsPanel organizationId="org" serviceId="service" editable={editable} oidc /></QueryClientProvider>);
}
describe("Flash custom domains",()=>{
  beforeEach(()=>{vi.spyOn(api.flash.services,"listDomains").mockResolvedValue({items:[domain],provider_unavailable:false});});
  it("shows service-specific DNS proof and OIDC callback without enabling pending HTTPS",async()=>{
    panel(false);
    expect(await screen.findByText("app.example.org")).toBeVisible();
    expect(screen.getByText(domain.cname_target)).toBeVisible();
    expect(screen.getByText(domain.oidc_callback_url)).toBeVisible();
    expect(screen.queryByRole("link",{name:"https://app.example.org"})).toBeNull();
    expect(screen.queryByRole("button",{name:"ドメインを登録"})).toBeNull();
  });
  it("normalizes a hostname and requires an explicit delete confirmation",async()=>{
    const add=vi.spyOn(api.flash.services,"addDomain").mockResolvedValue(domain);
    const remove=vi.spyOn(api.flash.services,"deleteDomain").mockResolvedValue({id:domain.id,phase:"deleting"});
    const user=userEvent.setup();panel();
    await screen.findByText("app.example.org");
    await user.type(screen.getByRole("textbox",{name:"独自ドメイン"}),"APP.example.org.");
    await user.click(screen.getByRole("button",{name:"ドメインを登録"}));
    expect(add).toHaveBeenCalledWith("org","service","app.example.org");
    await user.click(screen.getByRole("button",{name:"削除"}));
    expect(remove).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button",{name:"このドメインを削除"}));
    expect(remove).toHaveBeenCalledWith("org","service","domain-one");
  });
});
