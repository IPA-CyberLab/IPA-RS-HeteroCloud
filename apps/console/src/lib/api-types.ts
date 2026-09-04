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
  owner_console: boolean;
}

export interface FlowQuotaLimits {
  max_services: number;
  max_rooms_per_service: number;
  max_total_rooms: number;
  max_participants_per_service: number;
  max_rate_limit_requests_per_second: number;
  max_rate_limit_burst: number;
  max_developer_credentials_per_service: number;
}

export interface FlashQuotaLimits {
  max_services: number;
  max_replicas_per_service: number;
  max_cpu_millis_per_vm: number;
  max_memory_mib_per_vm: number;
  max_disk_gib_per_vm: number;
  max_total_replicas: number;
  max_total_cpu_millis: number;
  max_total_memory_mib: number;
  max_total_disk_gib: number;
}

export interface RegistryQuotaLimits {
  storage_gib: number;
  max_credentials: number;
}

export interface SyouyuQuotaLimits {
  max_buckets: number;
  max_bytes_per_bucket: number;
  max_objects_per_bucket: number;
  max_total_bytes: number;
  max_credentials_per_bucket: number;
  max_total_credentials: number;
}

export interface ResourceQuotaLimits {
  flow: FlowQuotaLimits;
  flash: FlashQuotaLimits;
  registry: RegistryQuotaLimits;
  syouyu: SyouyuQuotaLimits;
}

export interface ResourceQuotaUsage {
  flow_services: number;
  flow_max_rooms_per_service: number;
  flow_configured_rooms: number;
  flow_max_participants_per_service: number;
  flow_max_rate_limit_requests_per_second: number;
  flow_max_rate_limit_burst: number;
  flow_developer_credentials: number;
  flow_max_developer_credentials_per_service: number;
  flash_services: number;
  flash_max_replicas_per_service: number;
  flash_max_cpu_millis_per_vm: number;
  flash_max_memory_mib_per_vm: number;
  flash_max_disk_gib_per_vm: number;
  flash_replicas: number;
  flash_cpu_millis: number;
  flash_memory_mib: number;
  flash_disk_gib: number;
  registry_storage_bytes: number | null;
  registry_credentials: number;
  syouyu_buckets: number;
  syouyu_max_bytes_per_bucket: number;
  syouyu_max_objects_per_bucket: number;
  syouyu_configured_bytes: number;
  syouyu_storage_bytes: number | null;
  syouyu_credentials: number;
}

export interface ResourceQuotaTenant {
  organization: Organization;
  override_limits: ResourceQuotaLimits | null;
  effective_limits: ResourceQuotaLimits;
  usage: ResourceQuotaUsage;
}

export interface OwnerQuotaOverview {
  defaults: ResourceQuotaLimits;
  tenants: ResourceQuotaTenant[];
}

export interface UserExternalIdentity {
  issuer: string;
  subject: string;
  created_at: string;
}

export interface UserLoginEvent {
  id: number;
  user_id: string;
  source_ip: string | null;
  authentication_method: "local" | "oidc";
  occurred_at: string;
}

export interface OwnerAccount {
  user: CloudUser;
  has_local_password: boolean;
  external_identities: UserExternalIdentity[];
  memberships: Membership[];
  last_login: UserLoginEvent | null;
  login_count: number;
}

export interface RegistryCredential {
  id: string;
  name: string;
  username: string | null;
  status: "active";
  created_at: string;
}

export interface RegistryStatus {
  endpoint: string;
  project: string;
  image_prefix: string;
  storage_limit_bytes: number;
  storage_used_bytes: number;
  max_credentials: number;
  credentials: RegistryCredential[];
}

export interface RegistryImage {
  reference: string;
  repository: string;
  tag: string | null;
  digest: string;
  size_bytes: number;
  pushed_at: string | null;
}

export interface RegistryImageDeleteResult {
  storage_used_bytes: number;
}

export interface RegistryCredentialSecret {
  credential: RegistryCredential;
  username: string;
  password: string;
  login_host: string;
  login_command: string;
  image_prefix: string;
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

export type ServiceState =
  | "provisioning"
  | "ready"
  | "updating"
  | "deleting"
  | "error";

export interface RealtimeServiceSpec {
  region: string;
  max_participants: number;
  max_rooms: number;
  rate_limit: {
    requests_per_second: number;
    burst: number;
  };
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

export type FlashPortProtocol = "tcp" | "udp";

export interface FlashPortInput {
  name: string;
  protocol: FlashPortProtocol;
  container_port: number;
}

export interface FlashPort extends FlashPortInput {
  service_port: number;
}

export interface FlashExposure {
  type: "internal" | "public";
  traffic_mode: "forwarded" | "direct";
  allowed_source_cidrs?: string[];
  denied_source_cidrs?: string[];
}

export interface FlashServiceSpec {
  region: string;
  image: string;
  replicas: number;
  cpu_millis: number;
  memory_mib: number;
  ephemeral_storage_gib: number;
  ports: FlashPort[];
  exposure: FlashExposure;
  env: Record<string, string>;
  command: string[];
  args: string[];
  metadata: Record<string, unknown>;
}

export interface FlashServiceSpecInput extends Omit<FlashServiceSpec, "ports"> {
  ports: FlashPortInput[];
}

export interface FlashServiceEndpoint {
  name?: string;
  protocol?: FlashPortProtocol | Uppercase<FlashPortProtocol>;
  host?: string;
  address?: string;
  port?: number;
  url?: string;
}

export interface FlashServiceStatus {
  [key: string]: unknown;
  operation_id?: string;
  status?: FlashServiceStatus;
  observed_generation?: number;
  ready_replicas?: number;
  available_replicas?: number;
  runtime_class?: string;
  message?: string;
  endpoints?: FlashServiceEndpoint[] | Record<string, unknown>;
}

export interface FlashService {
  id: string;
  organization_id: string;
  project_id: string;
  provider: "flash";
  name: string;
  generation: number;
  state: ServiceState;
  spec: FlashServiceSpec;
  status: FlashServiceStatus;
  created_at: string;
  updated_at: string;
}

export interface SyouyuBucketSpec {
  region: string;
  bucket_name: string;
  quota_bytes: number;
  quota_objects: number;
  metadata: Record<string, unknown>;
}

export interface SyouyuBucketStatus {
  [key: string]: unknown;
  phase?: "provisioning" | "ready" | "degraded" | "error" | "deleting";
  observed_generation?: number;
  operation_id?: string;
  endpoint?: string;
  bucket_id?: string;
  bucket_name?: string;
  bytes?: number;
  objects?: number;
  credentials?: number;
  message?: string;
}

export interface SyouyuBucket {
  id: string;
  organization_id: string;
  project_id: string;
  provider: "syouyu";
  name: string;
  generation: number;
  state: ServiceState;
  spec: SyouyuBucketSpec;
  status: SyouyuBucketStatus;
  created_at: string;
  updated_at: string;
}

export interface CreateSyouyuBucketRequest {
  project_id: string;
  name: string;
  spec: SyouyuBucketSpec;
}

export interface UpdateSyouyuBucketRequest {
  name: string;
  spec: SyouyuBucketSpec;
}

export type SyouyuPermission = "list" | "read" | "write" | "delete";

export interface SyouyuCredential {
  id: string;
  service_instance_id: string;
  name: string;
  access_key_id: string;
  permissions: SyouyuPermission[];
  prefix: string | null;
  status: "active" | "revoked";
  created_at: string;
  revoked_at: string | null;
}

export interface CreateSyouyuCredentialRequest {
  name: string;
  permissions: SyouyuPermission[];
  prefix?: string;
}

export interface SyouyuCredentialSecret {
  credential: SyouyuCredential;
  secret_access_key: string;
  endpoint: string;
  region: string;
  bucket: string;
}

export interface SyouyuUsage {
  quota_bytes: number;
  quota_objects: number;
  used_bytes: number;
  object_count: number;
  unfinished_upload_bytes: number;
  credential_count: number;
}

export interface FlashContainer {
  name: string;
  phase: string;
  ready: boolean;
}

export interface FlashContainerList {
  items: FlashContainer[];
}

export interface CreateFlashServiceRequest {
  project_id: string;
  name: string;
  spec: FlashServiceSpecInput;
}

export interface UpdateFlashServiceRequest {
  name: string;
  spec: FlashServiceSpecInput;
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
