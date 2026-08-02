export type UserStatus = "active" | "suspended";

export interface CloudUser {
  id: string;
  email: string;
  display_name: string;
  status: UserStatus;
  created_at: string;
}

export interface Membership {
  organization_id: string;
  organization_slug: string;
  organization_name: string;
  principal_id: string;
  role: "owner" | "member";
}

export interface Session {
  user: CloudUser;
  memberships: Membership[];
  csrf_token: string;
}

export interface LoginRequest {
  email: string;
  password: string;
}

export interface RegisterRequest {
  invitation_code: string;
  email: string;
  display_name: string;
  password: string;
}

export interface CollectionResponse<T> {
  items: T[];
}

export interface Organization {
  id: string;
  slug: string;
  name: string;
  created_at: string;
}

export interface Project {
  id: string;
  organization_id: string;
  slug: string;
  name: string;
  created_at: string;
}

export interface CreateProjectRequest {
  slug: string;
  name: string;
}

export type PrincipalKind = "user" | "service_account";

export interface Principal {
  id: string;
  organization_id: string;
  kind: PrincipalKind;
  name: string;
  user_id: string | null;
  enabled: boolean;
  created_at: string;
}

export interface CreateServiceAccountRequest {
  name: string;
}

export type PolicyEffect = "Allow" | "Deny";

export interface PolicyStatement {
  effect: PolicyEffect;
  actions: string[];
  resources: string[];
}

export interface PolicyDocument {
  version: "2026-07-31";
  statements: PolicyStatement[];
}

export interface IamPolicy {
  id: string;
  organization_id: string;
  name: string;
  document: PolicyDocument;
  semantics_digest: string;
  created_at: string;
  updated_at: string;
}

export interface CreatePolicyRequest {
  name: string;
  document: PolicyDocument;
}

export interface CreateBindingRequest {
  principal_id: string;
  policy_id: string;
}

export interface BindingResponse {
  id: string;
}

export interface CreateInvitationRequest {
  expires_in_hours: number;
}

export interface InvitationResponse {
  id: string;
  code: string;
  max_uses: number;
  expires_at: string;
}

export type TrafficMode = "direct" | "forwarded";
export type ServiceState =
  | "provisioning"
  | "ready"
  | "updating"
  | "deleting"
  | "error";

export interface RealtimeServiceSpec {
  region: string;
  traffic_mode: TrafficMode;
  max_participants: number;
  max_rooms: number;
  rate_limit: {
    requests_per_second: number;
    burst: number;
  };
  turn_enabled: boolean;
  metadata: Record<string, unknown>;
}

export interface RealtimeServiceEndpoints {
  api: string[];
  signaling: string[];
  livekit: string[];
  stun: string[];
  turn: string[];
}

export interface RealtimeService {
  id: string;
  organization_id: string;
  project_id: string;
  provider: "flow";
  name: string;
  generation: number;
  state: ServiceState;
  spec: RealtimeServiceSpec;
  status: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface CreateRealtimeServiceRequest {
  project_id: string;
  name: string;
  spec: RealtimeServiceSpec;
}

export interface UpdateRealtimeServiceRequest {
  name?: string;
  spec?: RealtimeServiceSpec;
}

export interface RealtimeServiceMetrics {
  active_rooms: number;
  concurrent_connections: number;
  ingress_bytes: number;
  egress_bytes: number;
  transferred_bytes: number;
  measured_at: string;
  sfu_participants: number;
  p2p_connections: number;
  room_limit: number | null;
  endpoints: RealtimeServiceEndpoints;
}

export type RealtimeMetricsRange = "1h" | "6h" | "24h" | "7d" | "30d";

export interface RealtimeServiceMetricSample {
  sampled_at: string;
  active_rooms: number;
  concurrent_connections: number;
  ingress_bytes: number;
  egress_bytes: number;
  transferred_bytes: number;
}

export interface RealtimeServiceMetricHistory {
  range: RealtimeMetricsRange;
  step_seconds: number;
  samples: RealtimeServiceMetricSample[];
}

export interface CreateRealtimeAccessCredentialRequest {
  permissions: string[];
  expires_in_seconds?: number;
}

export interface RealtimeAccessCredential {
  context_id: string;
  organization_id: string;
  project_id: string;
  service_instance_id: string;
  principal_id: string;
  issued_at: string | number;
  expires_at: string | number;
  headers: Record<string, string>;
  endpoints: string[];
  rate_limit: {
    requests_per_second: number;
    burst: number;
  };
}

export interface RealtimeDeveloperCredential {
  id: string;
  name: string;
  prefix: string;
  permissions: string[];
  expires_at: string;
  last_used_at: string | null;
  revoked_at: string | null;
  created_at: string;
}

export interface CreateRealtimeDeveloperCredentialRequest {
  name: string;
  expires_in_days: number;
  permissions: string[];
}

export interface RealtimeDeveloperCredentialSecret
  extends RealtimeDeveloperCredential {
  credential: string;
  mint_endpoint: string;
}

export interface RealtimeAccessContext {
  context_id: string;
  credential_id: string | null;
  principal_id: string;
  permissions: string[];
  issued_at: string;
  expires_at: string;
  revoked_at: string | null;
}

export type AuditDecision = "allow" | "deny" | "error";

export interface AuditEvent {
  id: number;
  occurred_at: string;
  organization_id: string | null;
  principal_id: string | null;
  user_id: string | null;
  request_id: string;
  source_ip: string | null;
  action: string;
  resource: string;
  decision: AuditDecision;
  reason: string;
  metadata: Record<string, unknown>;
}

export interface ErrorEnvelope {
  error: {
    code: string;
    message: string;
  };
}
