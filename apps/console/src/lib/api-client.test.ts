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
    await client.realtime.services.list(organizationId);
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
      `${prefix}/realtime/services`,
      `${prefix}/audit-events?limit=500`,
      `${prefix}/iam/bindings`,
      `${prefix}/invitations`,
    ]);
  });

  it("Flowの管理API契約を使う", async () => {
    const serviceId = "0198a121-ffbd-70c2-a3c8-c65516d7b8fb";
    const service = {
      id: serviceId,
      organization_id: organizationId,
      project_id: "0198a11b-b519-7177-b6fd-bb131b5fb9e6",
      provider: "flow",
      name: "realtime-production",
      generation: 2,
      state: "ready",
      spec: {
        region: "heteronet-global",
        max_participants: 500,
        max_rooms: 100,
        rate_limit: {
          requests_per_second: 20,
          burst: 40,
        },
        metadata: {},
      },
      status: {},
      created_at: "2026-08-01T08:00:00Z",
      updated_at: "2026-08-01T09:00:00Z",
    };
    const fetcher = vi.fn(
      async (input: string | URL | Request, init?: RequestInit) => {
        const path = String(input);
        if (path.endsWith("/auth/session")) return jsonResponse(session);
        if (path.includes("/metrics/history?range=")) {
          return jsonResponse({
            range: "24h",
            step_seconds: 900,
            samples: [
              {
                sampled_at: "2026-08-01T09:00:00Z",
                active_rooms: 2,
                concurrent_connections: 8,
                ingress_bytes: 1000,
                egress_bytes: 2000,
                transferred_bytes: 3000,
              },
            ],
          });
        }
        if (path.endsWith("/metrics")) {
          return jsonResponse({
            measured_at: "2026-08-01T09:00:00Z",
            active_rooms: 2,
            concurrent_connections: 8,
            sfu_participants: 6,
            p2p_connections: 2,
            ingress_bytes: 1000,
            egress_bytes: 2000,
            transferred_bytes: 3000,
            room_limit: 100,
            endpoints: {
              api: ["https://api.example.com"],
              signaling: ["wss://signal.example.com"],
              livekit: ["wss://livekit.example.com"],
              stun: ["stun:turn.example.com:3478"],
              turn: ["turn:turn.example.com:3478"],
            },
          });
        }
        if (path.endsWith("/access-credentials")) {
          return jsonResponse({
            context_id: "0198a122-ffbd-70c2-a3c8-c65516d7b8fb",
            headers: {
              "x-flow-principal": "principal-value",
              "x-flow-timestamp": "1754038800",
              "x-flow-signature": "signature-value",
            },
            endpoints: ["https://flow.example.com"],
            issued_at: 1754038800,
            expires_at: 1754042400,
            organization_id: organizationId,
            project_id: service.project_id,
            service_instance_id: serviceId,
            principal_id: session.memberships[0].principal_id,
            rate_limit: {
              requests_per_second: 20,
              burst: 40,
            },
          });
        }
        return jsonResponse(
          init?.method === "DELETE" ? { ...service, state: "deleting" } : service,
          init?.method === "DELETE" ? 202 : 200,
        );
      },
    ) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.session();
    await client.realtime.services.get(organizationId, serviceId);
    await client.realtime.services.update(organizationId, serviceId, {
      name: "realtime-primary",
    });
    await client.realtime.services.delete(organizationId, serviceId);
    await client.realtime.services.issueAccessCredential(
      organizationId,
      serviceId,
      {
        permissions: ["flow.room.join", "flow.metrics.read"],
        expires_in_seconds: 3600,
      },
    );
    await client.realtime.services.listDeveloperCredentials(
      organizationId,
      serviceId,
    );
    await client.realtime.services.createDeveloperCredential(
      organizationId,
      serviceId,
      {
        name: "production-backend",
        expires_in_days: 90,
        permissions: ["flow.room.join", "flow.signal.connect"],
      },
    );
    await client.realtime.services.rotateDeveloperCredential(
      organizationId,
      serviceId,
      "credential-1",
    );
    await client.realtime.services.revokeDeveloperCredential(
      organizationId,
      serviceId,
      "credential-1",
    );
    await client.realtime.services.listAccessContexts(organizationId, serviceId);
    await client.realtime.services.revokeAccessContext(
      organizationId,
      serviceId,
      "context-1",
    );
    await client.realtime.services.metrics(organizationId, serviceId);
    await client.realtime.services.metricsHistory(
      organizationId,
      service.project_id,
      serviceId,
      "24h",
    );

    const calls = vi.mocked(fetcher).mock.calls.slice(1);
    const base = `/api/v1/organizations/${organizationId}/realtime/services/${serviceId}`;
    const history = `/api/v1/organizations/${organizationId}/projects/${service.project_id}/realtime/services/${serviceId}/metrics/history?range=24h`;
    expect(calls.map(([url]) => String(url))).toEqual([
      base,
      base,
      base,
      `${base}/access-credentials`,
      `${base}/developer-credentials`,
      `${base}/developer-credentials`,
      `${base}/developer-credentials/credential-1/rotate`,
      `${base}/developer-credentials/credential-1`,
      `${base}/access-contexts?limit=100`,
      `${base}/access-contexts/context-1`,
      `${base}/metrics`,
      history,
    ]);
    expect(calls.map(([, options]) => options?.method ?? "GET")).toEqual([
      "GET",
      "PATCH",
      "DELETE",
      "POST",
      "GET",
      "POST",
      "POST",
      "DELETE",
      "GET",
      "DELETE",
      "GET",
      "GET",
    ]);
    expect(JSON.parse(String(calls[1][1]?.body))).toEqual({
      name: "realtime-primary",
    });
    expect(JSON.parse(String(calls[3][1]?.body))).toEqual({
      permissions: ["flow.room.join", "flow.metrics.read"],
      expires_in_seconds: 3600,
    });
    expect(JSON.parse(String(calls[5][1]?.body))).toEqual({
      name: "production-backend",
      expires_in_days: 90,
      permissions: ["flow.room.join", "flow.signal.connect"],
    });
    expect(JSON.parse(String(calls[6][1]?.body))).toEqual({});
    calls
      .filter(([, options]) => ["PATCH", "DELETE", "POST"].includes(options?.method ?? ""))
      .forEach(([, options]) => {
        expect(new Headers(options?.headers).get("x-heterocloud-csrf")).toBe(
          session.csrf_token,
        );
      });
  });

  it("Flashの管理API契約をGET/POST/PUT/DELETEで使う", async () => {
    const serviceId = "flash-service-1";
    const input = {
      project_id: "project-1",
      name: "game-server",
      spec: {
        region: "heteronet-global",
        image: "ghcr.io/example/game-server:v1",
        replicas: 3,
        cpu_millis: 1_000,
        memory_mib: 2_048,
        ephemeral_storage_gib: 20,
        ports: [
          {
            name: "game",
            protocol: "udp" as const,
            container_port: 7777,
            service_port: 7777,
          },
        ],
        exposure: { type: "public" as const, traffic_mode: "forwarded" as const },
        env: { GAME_MODE: "production" },
        command: ["/app/server"],
        args: ["--listen", "0.0.0.0:7777"],
        metadata: {},
      },
    };
    const service = {
      id: serviceId,
      organization_id: organizationId,
      provider: "flash",
      generation: 1,
      state: "provisioning",
      status: {},
      created_at: "2026-08-21T08:00:00Z",
      updated_at: "2026-08-21T08:00:00Z",
      ...input,
    };
    const fetcher = vi.fn(
      async (request: string | URL | Request) => {
        if (String(request).endsWith("/auth/session")) return jsonResponse(session);
        if (String(request).endsWith("/flash/services")) {
          return jsonResponse({ items: [service] });
        }
        return jsonResponse(service);
      },
    ) as unknown as typeof fetch;
    const client = new HeteroCloudApiClient("/api/v1", fetcher);

    await client.auth.session();
    await client.flash.services.list(organizationId);
    await client.flash.services.create(organizationId, input);
    await client.flash.services.get(organizationId, serviceId);
    await client.flash.services.update(organizationId, serviceId, {
      name: input.name,
      spec: input.spec,
    });
    await client.flash.services.delete(organizationId, serviceId);

    const calls = vi.mocked(fetcher).mock.calls.slice(1);
    const collection = `/api/v1/organizations/${organizationId}/flash/services`;
    const item = `${collection}/${serviceId}`;
    expect(calls.map(([url]) => String(url))).toEqual([
      collection,
      collection,
      item,
      item,
      item,
    ]);
    expect(calls.map(([, options]) => options?.method ?? "GET")).toEqual([
      "GET",
      "POST",
      "GET",
      "PUT",
      "DELETE",
    ]);
    expect(JSON.parse(String(calls[1][1]?.body))).toEqual(input);
    expect(JSON.parse(String(calls[3][1]?.body))).toEqual({
      name: input.name,
      spec: input.spec,
    });
    calls
      .filter(([, options]) => ["POST", "PUT", "DELETE"].includes(options?.method ?? ""))
      .forEach(([, options]) => {
        expect(new Headers(options?.headers).get("x-heterocloud-csrf")).toBe(
          session.csrf_token,
        );
      });
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
