import type {
  AuditEvent,
  BindingResponse,
  CollectionResponse,
  CreateBindingRequest,
  CreateInvitationRequest,
  CreatePolicyRequest,
  CreateProjectRequest,
  CreateRealtimeAccessCredentialRequest,
  CreateRealtimeServiceRequest,
  CreateServiceAccountRequest,
  ErrorEnvelope,
  IamPolicy,
  InvitationResponse,
  LoginRequest,
  Organization,
  Principal,
  Project,
  RealtimeAccessCredential,
  RealtimeService,
  RealtimeServiceMetrics,
  RegisterRequest,
  Session,
  UpdateRealtimeServiceRequest,
} from "@/lib/api-types";

const API_BASE_URL = "/api/v1";
const CSRF_HEADER = "x-heterocloud-csrf";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(
    message: string,
    {
      status = 0,
      code = "unknown_error",
    }: {
      status?: number;
      code?: string;
    } = {},
  ) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

type RequestOptions = Omit<RequestInit, "body"> & {
  body?: unknown;
};

function queryString(params: Record<string, string | number | undefined>) {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined) search.set(key, String(value));
  });
  const query = search.toString();
  return query ? `?${query}` : "";
}

function organizationPath(organizationId: string, suffix: string): string {
  return `/organizations/${encodeURIComponent(organizationId)}/${suffix}`;
}

async function parseError(
  response: Response,
): Promise<ErrorEnvelope["error"] | null> {
  const contentType = response.headers.get("content-type") ?? "";
  if (
    !contentType.includes("application/json") &&
    !contentType.includes("+json")
  ) {
    return null;
  }

  try {
    const value = (await response.json()) as unknown;
    if (
      typeof value === "object" &&
      value !== null &&
      "error" in value &&
      typeof value.error === "object" &&
      value.error !== null &&
      "code" in value.error &&
      typeof value.error.code === "string" &&
      "message" in value.error &&
      typeof value.error.message === "string"
    ) {
      return {
        code: value.error.code,
        message: value.error.message,
      };
    }
    return null;
  } catch {
    return null;
  }
}

export class HeteroCloudApiClient {
  private csrfToken: string | null = null;

  constructor(
    private readonly baseUrl = API_BASE_URL,
    private readonly fetcher: typeof fetch = globalThis.fetch.bind(globalThis),
  ) {}

  private rememberSession(session: Session): Session {
    this.csrfToken = session.csrf_token;
    return session;
  }

  private async request<T>(path: string, options: RequestOptions = {}): Promise<T> {
    const requestOptions = options;
    const headers = new Headers(requestOptions.headers);
    headers.set("Accept", "application/json");
    headers.set("X-Requested-With", "XMLHttpRequest");

    const method = (requestOptions.method ?? "GET").toUpperCase();
    const mutation = !["GET", "HEAD", "OPTIONS"].includes(method);
    const publicAuthMutation =
      path === "/auth/login" || path === "/auth/register";
    if (mutation && !publicAuthMutation) {
      if (!this.csrfToken) {
        throw new ApiError("CSRFトークンがありません。再ログインしてください。", {
          code: "missing_csrf_token",
        });
      }
      headers.set(CSRF_HEADER, this.csrfToken);
    }

    let body: BodyInit | undefined;
    if (options.body !== undefined) {
      headers.set("Content-Type", "application/json");
      body = JSON.stringify(options.body);
    }

    let response: Response;
    try {
      response = await this.fetcher(`${this.baseUrl}${path}`, {
        ...requestOptions,
        headers,
        body,
        credentials: "include",
        // The browser owns the Origin header and emits it for same-origin POSTs.
        mode: "same-origin",
      });
    } catch {
      throw new ApiError("APIに接続できませんでした。接続状態を確認してください。", {
        code: "network_error",
      });
    }

    if (!response.ok) {
      if (response.status === 401) this.csrfToken = null;
      const error = await parseError(response);
      throw new ApiError(
        error?.message ?? `APIがエラーを返しました (${response.status})`,
        {
          status: response.status,
          code: error?.code ?? "api_error",
        },
      );
    }

    if (response.status === 204) return undefined as T;

    const contentType = response.headers.get("content-type") ?? "";
    if (
      !contentType.includes("application/json") &&
      !contentType.includes("+json")
    ) {
      throw new ApiError("APIレスポンスの形式が正しくありません。", {
        status: response.status,
        code: "invalid_response",
      });
    }

    try {
      return (await response.json()) as T;
    } catch {
      throw new ApiError("APIレスポンスを読み取れませんでした。", {
        status: response.status,
        code: "invalid_response",
      });
    }
  }

  readonly auth = {
    session: async (signal?: AbortSignal) => {
      const session = await this.request<Session>("/auth/session", { signal });
      return this.rememberSession(session);
    },
    login: async (input: LoginRequest) => {
      const session = await this.request<Session>("/auth/login", {
        method: "POST",
        body: input,
      });
      return this.rememberSession(session);
    },
    register: async (input: RegisterRequest) => {
      const session = await this.request<Session>("/auth/register", {
        method: "POST",
        body: input,
      });
      return this.rememberSession(session);
    },
    logout: async () => {
      await this.request<void>("/auth/logout", {
        method: "POST",
      });
      this.csrfToken = null;
    },
  };

  readonly organizations = {
    list: (signal?: AbortSignal) =>
      this.request<CollectionResponse<Organization>>("/organizations", {
        signal,
      }),
  };

  readonly projects = {
    list: (organizationId: string, signal?: AbortSignal) =>
      this.request<CollectionResponse<Project>>(
        organizationPath(organizationId, "projects"),
        { signal },
      ),
    create: (organizationId: string, input: CreateProjectRequest) =>
      this.request<Project>(organizationPath(organizationId, "projects"), {
        method: "POST",
        body: input,
      }),
  };

  readonly iam = {
    principals: {
      list: (organizationId: string, signal?: AbortSignal) =>
        this.request<CollectionResponse<Principal>>(
          organizationPath(organizationId, "iam/principals"),
          { signal },
        ),
      createServiceAccount: (
        organizationId: string,
        input: CreateServiceAccountRequest,
      ) =>
        this.request<Principal>(
          organizationPath(organizationId, "iam/principals"),
          {
            method: "POST",
            body: input,
          },
        ),
    },
    policies: {
      list: (organizationId: string, signal?: AbortSignal) =>
        this.request<CollectionResponse<IamPolicy>>(
          organizationPath(organizationId, "iam/policies"),
          { signal },
        ),
      create: (organizationId: string, input: CreatePolicyRequest) =>
        this.request<IamPolicy>(
          organizationPath(organizationId, "iam/policies"),
          {
            method: "POST",
            body: input,
          },
        ),
    },
    bindings: {
      create: (organizationId: string, input: CreateBindingRequest) =>
        this.request<BindingResponse>(
          organizationPath(organizationId, "iam/bindings"),
          {
            method: "POST",
            body: input,
          },
        ),
    },
  };

  readonly invitations = {
    create: (organizationId: string, input: CreateInvitationRequest) =>
      this.request<InvitationResponse>(
        organizationPath(organizationId, "invitations"),
        {
          method: "POST",
          body: input,
        },
      ),
  };

  readonly realtime = {
    services: {
      list: (
        organizationId: string,
        projectId?: string,
        signal?: AbortSignal,
      ) =>
        this.request<CollectionResponse<RealtimeService>>(
          `${organizationPath(organizationId, "realtime/services")}${queryString({
            project_id: projectId,
          })}`,
          { signal },
        ),
      create: (
        organizationId: string,
        input: CreateRealtimeServiceRequest,
      ) =>
        this.request<RealtimeService>(
          organizationPath(organizationId, "realtime/services"),
          {
            method: "POST",
            body: input,
          },
        ),
      get: (
        organizationId: string,
        serviceId: string,
        signal?: AbortSignal,
      ) =>
        this.request<RealtimeService>(
          organizationPath(
            organizationId,
            `realtime/services/${encodeURIComponent(serviceId)}`,
          ),
          { signal },
        ),
      update: (
        organizationId: string,
        serviceId: string,
        input: UpdateRealtimeServiceRequest,
      ) =>
        this.request<RealtimeService>(
          organizationPath(
            organizationId,
            `realtime/services/${encodeURIComponent(serviceId)}`,
          ),
          {
            method: "PATCH",
            body: input,
          },
        ),
      delete: (organizationId: string, serviceId: string) =>
        this.request<RealtimeService>(
          organizationPath(
            organizationId,
            `realtime/services/${encodeURIComponent(serviceId)}`,
          ),
          { method: "DELETE" },
        ),
      issueAccessCredential: (
        organizationId: string,
        serviceId: string,
        input: CreateRealtimeAccessCredentialRequest,
      ) =>
        this.request<RealtimeAccessCredential>(
          organizationPath(
            organizationId,
            `realtime/services/${encodeURIComponent(serviceId)}/access-credentials`,
          ),
          {
            method: "POST",
            body: input,
          },
        ),
      metrics: (
        organizationId: string,
        serviceId: string,
        signal?: AbortSignal,
      ) =>
        this.request<RealtimeServiceMetrics>(
          organizationPath(
            organizationId,
            `realtime/services/${encodeURIComponent(serviceId)}/metrics`,
          ),
          { signal },
        ),
    },
  };

  readonly auditEvents = {
    list: (organizationId: string, limit = 500, signal?: AbortSignal) =>
      this.request<CollectionResponse<AuditEvent>>(
        `${organizationPath(organizationId, "audit-events")}${queryString({
          limit,
        })}`,
        { signal },
      ),
  };
}

export const api = new HeteroCloudApiClient();

export function getApiErrorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  return "予期しないエラーが発生しました。";
}
