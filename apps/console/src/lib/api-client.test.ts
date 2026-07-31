import { describe, expect, it, vi } from "vitest";
import { ApiError, HeteroCloudApiClient } from "@/lib/api-client";
import type { Session } from "@/lib/api-types";

const organizationId = "0198a117-0d8c-70e2-a457-a83c253b9f21";
const session: Session = {
  user: {
    id: "0198a117-6ea7-7b49-9556-3a2f0dc43cc0",
    email: "admin@example.com",
    display_name: "Cloud Admin",
    status: "active",
    created_at: "2026-07-31T08:00:00Z",
  },
  memberships: [
    {
      organization_id: organizationId,
      organization_slug: "heterocloud-lab",
      organization_name: "HeteroCloud Lab",
      principal_id: "0198a118-073f-79e4-9ca4-0c1c2501c031",
      role: "owner",
    },
  ],
  csrf_token: "csrf-session-token",
};

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("HeteroCloudApiClient", () => {
  it("ブラウザのfetchを正しいglobal receiverで呼び出す", async () => {
    const originalFetch = globalThis.fetch;
    const receiverCheckingFetch = function (this: unknown) {
      if (this !== globalThis) throw new TypeError("Illegal invocation");
      return Promise.resolve(jsonResponse(session));
    } as unknown as typeof fetch;
    globalThis.fetch = receiverCheckingFetch;

    try {
      const client = new HeteroCloudApiClient("/api/v1");
      await expect(client.auth.session()).resolves.toEqual(session);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("同一originへCookie付きでセッションを要求する", async () => {
    const fetcher = vi.fn(async () => jsonResponse(session)) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await expect(client.auth.session()).resolves.toEqual(session);
    const [url, options] = vi.mocked(fetcher).mock.calls[0];
    expect(url).toBe("/api/v1/auth/session");
    expect(options?.credentials).toBe("include");
    expect(options?.mode).toBe("same-origin");
    expect(new Headers(options?.headers).get("X-Requested-With")).toBe(
      "XMLHttpRequest",
    );
  });

  it("ログインでCSRFを取得しlogoutへ自動付与する", async () => {
    const fetcher = vi.fn(
      async (_url: string | URL | Request, init?: RequestInit) =>
        init?.body ? jsonResponse(session) : new Response(null, { status: 204 }),
    ) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.login({
      email: "admin@example.com",
      password: "correct-horse-battery-staple",
    });
    const [loginUrl, loginOptions] = vi.mocked(fetcher).mock.calls[0];
    expect(loginUrl).toBe("/api/v1/auth/login");
    expect(loginOptions?.method).toBe("POST");
    expect(loginOptions?.mode).toBe("same-origin");
    expect(
      new Headers(loginOptions?.headers).has("x-heterocloud-csrf"),
    ).toBe(false);
    expect(JSON.parse(String(loginOptions?.body))).toEqual({
      email: "admin@example.com",
      password: "correct-horse-battery-staple",
    });

    await client.auth.logout();
    const [logoutUrl, logoutOptions] = vi.mocked(fetcher).mock.calls[1];
    expect(logoutUrl).toBe("/api/v1/auth/logout");
    expect(logoutOptions?.method).toBe("POST");
    expect(
      new Headers(logoutOptions?.headers).get("x-heterocloud-csrf"),
    ).toBe(session.csrf_token);
  });

  it("招待登録を公開POST契約で送り、返されたsessionを保持する", async () => {
    const fetcher = vi.fn(async () => jsonResponse(session)) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.register({
      invitation_code: "invite-secret",
      email: "new@example.com",
      display_name: "New Member",
      password: "correct-horse-battery-staple",
    });

    const [url, options] = vi.mocked(fetcher).mock.calls[0];
    expect(url).toBe("/api/v1/auth/register");
    expect(options?.method).toBe("POST");
    expect(
      new Headers(options?.headers).has("x-heterocloud-csrf"),
    ).toBe(false);
    expect(JSON.parse(String(options?.body))).toEqual({
      invitation_code: "invite-secret",
      email: "new@example.com",
      display_name: "New Member",
      password: "correct-horse-battery-staple",
    });
  });

  it("組織スコープURLとCSRFをresource mutationへ付与する", async () => {
    const project = {
      id: "0198a11b-b519-7177-b6fd-bb131b5fb9e6",
      organization_id: organizationId,
      slug: "realtime-prod",
      name: "Realtime Production",
      created_at: "2026-07-31T08:00:00Z",
    };
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(session))
      .mockResolvedValueOnce(jsonResponse(project)) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.session();
    await client.projects.create(organizationId, {
      slug: "realtime-prod",
      name: "Realtime Production",
    });

    const [url, options] = vi.mocked(fetcher).mock.calls[1];
    expect(url).toBe(
      `/api/v1/organizations/${organizationId}/projects`,
    );
    expect(options?.method).toBe("POST");
    expect(new Headers(options?.headers).get("x-heterocloud-csrf")).toBe(
      session.csrf_token,
    );
  });

  it("すべての組織スコープ資源を契約済みパスへ送る", async () => {
    const fetcher = vi.fn(
      async (input: string | URL | Request, init?: RequestInit) => {
        const path = String(input);
        if (path.endsWith("/auth/session")) return jsonResponse(session);
        if (init?.method === "POST") {
          return jsonResponse({ id: "0198a11e-ffbd-70c2-a3c8-c65516d7b8fb" });
        }
        return jsonResponse({ items: [] });
      },
    ) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.session();
    await client.projects.list(organizationId);
    await client.iam.principals.list(organizationId);
    await client.iam.policies.list(organizationId);
    await client.flow.instances.list(organizationId);
    await client.auditEvents.list(organizationId, 500);
    await client.iam.bindings.create(organizationId, {
      principal_id: "0198a11f-3bf3-7310-bd79-e27183663d05",
      policy_id: "0198a11f-5b26-7050-84d5-7982145e9042",
    });
    await client.invitations.create(organizationId, {
      expires_in_hours: 24,
    });

    const urls = vi
      .mocked(fetcher)
      .mock.calls.slice(1)
      .map(([url]) => String(url));
    const prefix = `/api/v1/organizations/${organizationId}`;
    expect(urls).toEqual([
      `${prefix}/projects`,
      `${prefix}/iam/principals`,
      `${prefix}/iam/policies`,
      `${prefix}/flow/instances`,
      `${prefix}/audit-events?limit=500`,
      `${prefix}/iam/bindings`,
      `${prefix}/invitations`,
    ]);
  });

  it("セッション取得前のcookie mutationを送信しない", async () => {
    const fetcher = vi.fn() as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await expect(
      client.invitations.create(organizationId, {
        expires_in_hours: 24,
      }),
    ).rejects.toMatchObject({
      code: "missing_csrf_token",
    });
    expect(fetcher).not.toHaveBeenCalled();
  });

  it("API障害時に偽データへフォールバックせずエラーを返す", async () => {
    const fetcher = vi.fn(async () => {
      throw new TypeError("connection refused");
    }) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await expect(client.organizations.list()).rejects.toMatchObject({
      name: "ApiError",
      code: "network_error",
    });
  });

  it("Rust APIのerror envelopeを型付きエラーへ変換する", async () => {
    const fetcher = vi.fn(async () =>
      jsonResponse(
        {
          error: {
            code: "forbidden",
            message: "The authenticated principal is not authorized.",
          },
        },
        403,
      ),
    ) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    const promise = client.projects.list(organizationId);
    await expect(promise).rejects.toBeInstanceOf(ApiError);
    await expect(promise).rejects.toMatchObject({
      status: 403,
      code: "forbidden",
      message: "The authenticated principal is not authorized.",
    });
  });
});
