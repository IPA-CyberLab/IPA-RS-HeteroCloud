import { Alert } from "@/components/ui";
import { Box } from "@/components/ui";
import { Button } from "@/components/ui";
import { ColumnLayout } from "@/components/ui";
import { Container } from "@/components/ui";
import { FormField } from "@/components/ui";
import { Header } from "@/components/ui";
import { Input } from "@/components/ui";
import { Modal } from "@/components/ui";
import { Select } from "@/components/ui";
import { SpaceBetween } from "@/components/ui";
import { Textarea } from "@/components/ui";
import { Toggle } from "@/components/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, Globe2, Network, ShieldCheck, Trash2 } from "lucide-react";
import { useState } from "react";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { ErrorState } from "@/components/shared/error-state";
import { ProjectSelector } from "@/components/shared/resource-selectors";
import { useActiveOrganization } from "@/features/organizations/organization-context";
import { api, getApiErrorMessage } from "@/lib/api-client";
import type { VpcNetwork, VpcPeer, VpcRule, VpcSpec } from "@/lib/api-types";
import { vpcsQueryOptions, flashServicesQueryOptions } from "@/lib/queries";
import "./vpc-page.css";

const initial: VpcSpec = {
  region: "heteronet-global",
  description: "",
  nat: { enabled: false },
  security_groups: ["default"],
  rules: [],
};
const validGroup = (name: string) =>
  name.length <= 63 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(name);
const peerValue = (p: VpcPeer) =>
  p.type === "all"
    ? "all"
    : p.type === "service"
      ? `service:${p.service_id}`
      : `group:${p.name}`;
const parsePeer = (value: string): VpcPeer =>
  value === "all"
    ? { type: "all" }
    : value.startsWith("service:")
      ? { type: "service", service_id: value.slice(8) }
      : { type: "security_group", name: value.slice(6) };
const peerLabel = (peer: VpcPeer, services: { id: string; name: string }[]) =>
  peer.type === "all"
    ? "VPC内のすべて"
    : peer.type === "security_group"
      ? peer.name
      : (services.find((s) => s.id === peer.service_id)?.name ??
        peer.service_id);

function validateRules(rules: VpcRule[], groups: string[]): string | null {
  for (const [index, rule] of rules.entries()) {
    const peers = [rule.source, rule.destination];
    if (
      peers.some(
        (peer) => peer.type === "security_group" && !groups.includes(peer.name),
      )
    )
      return `ルール ${index + 1} に存在しないグループが指定されています。`;
    if (
      !Number.isInteger(rule.port) ||
      rule.port < 1 ||
      rule.port > 65535 ||
      (rule.end_port !== undefined &&
        (!Number.isInteger(rule.end_port) ||
          rule.end_port < rule.port ||
          rule.end_port > 65535))
    ) {
      return `ルール ${index + 1} のポート範囲を確認してください（1〜65535）。`;
    }
  }
  return null;
}

export function VpcPage() {
  const { activeOrganization } = useActiveOrganization();
  const org = activeOrganization.organization_id;
  const networks = useQuery(vpcsQueryOptions(org));
  const services = useQuery(flashServicesQueryOptions(org));
  const cache = useQueryClient();
  const [edit, setEdit] = useState<VpcNetwork | "new" | null>(null);
  const [deleting, setDeleting] = useState<VpcNetwork | null>(null);
  const [name, setName] = useState("");
  const [project, setProject] = useState("");
  const [spec, setSpec] = useState<VpcSpec>(initial);
  const [groups, setGroups] = useState<string[]>(["default"]);
  const [newGroup, setNewGroup] = useState("");
  const [validation, setValidation] = useState("");
  const invalidate = () =>
    cache.invalidateQueries({ queryKey: ["organizations", org, "vpc"] });
  const save = useMutation({
    mutationFn: () => {
      const next = { ...spec, security_groups: groups };
      return edit && edit !== "new"
        ? api.vpc.update(org, edit.id, { name: name.trim(), spec: next })
        : api.vpc.create(org, {
            project_id: project,
            name: name.trim(),
            spec: next,
          });
    },
    onSuccess: async () => {
      setEdit(null);
      await invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: (id: string) => api.vpc.delete(org, id),
    onSuccess: async () => {
      setDeleting(null);
      await invalidate();
    },
  });
  function open(v: VpcNetwork | "new") {
    save.reset();
    setValidation("");
    setNewGroup("");
    setEdit(v);
    setName(v === "new" ? "" : v.name);
    setProject(v === "new" ? "" : v.project_id);
    setSpec(v === "new" ? structuredClone(initial) : structuredClone(v.spec));
    setGroups(v === "new" ? ["default"] : [...v.spec.security_groups]);
  }
  function changeRule(index: number, patch: Partial<VpcRule>) {
    setSpec((current) => ({
      ...current,
      rules: current.rules.map((r, i) =>
        i === index ? { ...r, ...patch } : r,
      ),
    }));
  }
  function addGroup() {
    const value = newGroup.trim();
    if (!validGroup(value)) {
      setValidation(
        "グループ名は英小文字・数字・ハイフンで入力してください。先頭と末尾にハイフンは使えません。",
      );
      return;
    }
    if (groups.includes(value)) {
      setValidation("同じ名前のグループが既にあります。");
      return;
    }
    setGroups((current) => [...current, value]);
    setNewGroup("");
    setValidation("");
  }
  function removeGroup(value: string) {
    if (
      spec.rules.some((rule) =>
        [rule.source, rule.destination].some(
          (peer) => peer.type === "security_group" && peer.name === value,
        ),
      )
    ) {
      setValidation(
        `「${value}」は通信ルールで使用されています。先にルールを変更してください。`,
      );
      return;
    }
    setGroups((current) => current.filter((group) => group !== value));
    setValidation("");
  }
  const peerOptions = [
    ...groups.map((g) => ({ value: `group:${g}`, label: `グループ: ${g}` })),
    ...(services.data?.items ?? [])
      .filter(
        (s) => edit && edit !== "new" && s.spec.network?.vpc_id === edit.id,
      )
      .map((s) => ({ value: `service:${s.id}`, label: `サービス: ${s.name}` })),
    { value: "all", label: "このVPCの全サービス" },
  ];
  if (networks.isPending) return <PageLoading />;
  if (networks.isError)
    return <ErrorState description={getApiErrorMessage(networks.error)} />;
  return (
    <SpaceBetween size="l">
      <PageHeader
        title="VPC"
        description="Flashサービス間のプライベート通信と外向きNATを管理します。"
        actions={
          <Button
            formAction="none"
            variant="primary"
            onClick={() => open("new")}
          >
            VPCを作成
          </Button>
        }
      />
      <div className="vpc-intro">
        <span className="vpc-intro__icon">
          <ShieldCheck size={19} aria-hidden="true" />
        </span>
        <div>
          <strong>プライベートなネットワーク</strong>
          <p>
            サービス間の通信はルールで許可します。NATは外向き接続専用で、外部には公開しません。
          </p>
        </div>
      </div>
      {services.isError && (
        <Alert
          type="warning"
          action={
            <Button onClick={() => void services.refetch()}>再試行</Button>
          }
        >
          接続サービスを取得できません。削除操作は確認できるまで利用できません。
        </Alert>
      )}
      {networks.data.items.length === 0 && (
        <div className="vpc-empty">
          <span className="vpc-empty__icon">
            <Network size={24} />
          </span>
          <strong>VPCがありません。</strong>
          <p>プライベートな通信を始めるにはVPCを作成してください。</p>
          <Button variant="primary" onClick={() => open("new")}>
            VPCを作成
          </Button>
        </div>
      )}
      {networks.data.items.map((v) => {
        const status = v.status.status;
        const attached = (services.data?.items ?? []).filter(
          (s) => s.spec.network?.vpc_id === v.id,
        );
        return (
          <Container
            key={v.id}
            header={
              <Header
                description={v.spec.description || "プライベートネットワーク"}
                actions={
                  <SpaceBetween direction="horizontal" size="xs">
                    <Button formAction="none" onClick={() => open(v)}>
                      編集
                    </Button>
                    <Button
                      formAction="none"
                      disabled={
                        services.isPending ||
                        services.isError ||
                        attached.length > 0 ||
                        v.state === "deleting"
                      }
                      onClick={() => {
                        remove.reset();
                        setDeleting(v);
                      }}
                    >
                      削除
                    </Button>
                  </SpaceBetween>
                }
              >
                <span className="vpc-heading">
                  <span className="vpc-heading__icon">
                    <Network size={19} aria-hidden="true" />
                  </span>
                  {v.name}
                </span>
              </Header>
            }
          >
            <SpaceBetween size="l">
              {v.status.observation === "unavailable" && (
                <Alert type="warning">
                  ネットワークの現在の状態を取得できません。
                </Alert>
              )}
              <div className="vpc-meta-grid">
                <div>
                  <span>状態</span>
                  <strong>
                    <i
                      className={`vpc-state-dot ${status?.phase === "ready" ? "is-ready" : ""}`}
                    />
                    {status?.phase ?? v.state}
                  </strong>
                </div>
                <div>
                  <span>外向きNAT</span>
                  <strong>{v.spec.nat.enabled ? "有効" : "無効"}</strong>
                </div>
                <div>
                  <span>接続サービス</span>
                  <strong>
                    {services.isPending || services.isError
                      ? "—"
                      : `${attached.length} 件`}
                  </strong>
                </div>
                <div>
                  <span>通信ルール</span>
                  <strong>{v.spec.rules.length} 件</strong>
                </div>
              </div>
              <div className="vpc-facts">
                <div>
                  <span>VPC ID</span>
                  <code>{v.id}</code>
                </div>
                {status?.dns_suffix && (
                  <div>
                    <span>内部DNS</span>
                    <code>名前.{status.dns_suffix}</code>
                  </div>
                )}
                {status?.nat_gateway_node && (
                  <div>
                    <span>NATゲートウェイ</span>
                    <span>
                      {status.nat_gateway_node}{" "}
                      <small>切替時に送信元IPが変わる場合があります</small>
                    </span>
                  </div>
                )}
              </div>
              <div className="vpc-section">
                <div className="vpc-section__title">
                  <ShieldCheck size={16} aria-hidden="true" />
                  <strong>セキュリティグループ</strong>
                </div>
                <div className="vpc-chip-list">
                  {v.spec.security_groups.map((group) => (
                    <span className="badge badge-outline" key={group}>
                      {group}
                    </span>
                  ))}
                </div>
              </div>
              <div className="vpc-section">
                <div className="vpc-section__title">
                  <ArrowRight size={16} aria-hidden="true" />
                  <strong>通信ルール</strong>
                  <span className="vpc-count">{v.spec.rules.length}</span>
                </div>
                {v.spec.rules.length ? (
                  <div className="vpc-rules">
                    {v.spec.rules.map((rule, index) => (
                      <div className="vpc-rule-summary" key={index}>
                        <span>{peerLabel(rule.source, attached)}</span>
                        <ArrowRight size={15} aria-label="から" />
                        <span>{peerLabel(rule.destination, attached)}</span>
                        <code>
                          {rule.protocol.toUpperCase()} {rule.port}
                          {rule.end_port && rule.end_port !== rule.port
                            ? `–${rule.end_port}`
                            : ""}
                        </code>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="vpc-muted">許可ルールはありません。</p>
                )}
              </div>
              <div className="vpc-section">
                <div className="vpc-section__title">
                  <Globe2 size={16} aria-hidden="true" />
                  <strong>接続サービス</strong>
                  <span className="vpc-count">
                    {services.isPending || services.isError
                      ? "—"
                      : attached.length}
                  </span>
                </div>
                {attached.length ? (
                  <div className="vpc-service-list">
                    {attached.map((s) => (
                      <div className="vpc-service-row" key={s.id}>
                        <a href={`/flash/services/${s.id}`}>{s.name}</a>
                        <span>
                          {s.spec.network?.private_name &&
                          s.spec.network.private_name !== s.name
                            ? s.spec.network.private_name
                            : ""}
                        </span>
                        <span>
                          {s.spec.network?.security_groups.join(", ")}
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="vpc-muted">
                    {services.isPending
                      ? "読み込み中"
                      : services.isError
                        ? "取得できません"
                        : "接続サービスはありません。"}
                  </p>
                )}
                {attached.length > 0 && (
                  <p className="vpc-muted">
                    削除するには、接続しているサービスを先に削除またはVPCから切り離してください。
                  </p>
                )}
              </div>
            </SpaceBetween>
          </Container>
        );
      })}
      <Modal
        visible={edit !== null}
        onDismiss={() => setEdit(null)}
        size="large"
        header={edit === "new" ? "VPCを作成" : "VPCを編集"}
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            setValidation("");
            if (!name.trim() || !project || !groups.length) {
              setValidation(
                "名前、プロジェクト、セキュリティグループを指定してください。",
              );
              return;
            }
            if (
              groups.some((group) => !validGroup(group)) ||
              new Set(groups).size !== groups.length
            ) {
              setValidation("セキュリティグループの名前を確認してください。");
              return;
            }
            const ruleError = validateRules(spec.rules, groups);
            if (ruleError) {
              setValidation(ruleError);
              return;
            }
            save.mutate();
          }}
        >
          <SpaceBetween size="l">
            {(validation || save.isError) && (
              <Alert type="error">
                {validation || getApiErrorMessage(save.error)}
              </Alert>
            )}
            <div className="vpc-editor-section">
              <div className="vpc-editor-section__heading">
                <span>01</span>
                <div>
                  <h3>基本設定</h3>
                  <p>ネットワークを識別する名前と所属先を設定します。</p>
                </div>
              </div>
              <SpaceBetween size="m">
                <ColumnLayout columns={2}>
                  <FormField label="名前">
                    <Input
                      value={name}
                      onChange={({ detail }) => setName(detail.value)}
                      disabled={save.isPending}
                    />
                  </FormField>
                  <FormField label="プロジェクト">
                    <ProjectSelector
                      value={project}
                      onValueChange={setProject}
                      disabled={edit !== "new" || save.isPending}
                    />
                  </FormField>
                </ColumnLayout>
                <ColumnLayout columns={2}>
                  <FormField label="リージョン">
                    <Input
                      value={spec.region}
                      onChange={({ detail }) =>
                        setSpec({ ...spec, region: detail.value })
                      }
                      disabled={save.isPending}
                    />
                  </FormField>
                  <FormField label="説明">
                    <Textarea
                      value={spec.description}
                      onChange={({ detail }) =>
                        setSpec({ ...spec, description: detail.value })
                      }
                      disabled={save.isPending}
                    />
                  </FormField>
                </ColumnLayout>
              </SpaceBetween>
            </div>
            <div className="vpc-editor-section">
              <div className="vpc-editor-section__heading">
                <span>02</span>
                <div>
                  <h3>外向きNAT</h3>
                  <p>
                    外部への接続に使用します。インターネットからの受信は許可しません。
                  </p>
                </div>
              </div>
              <div className="vpc-nat-control">
                <span className="vpc-nat-control__icon">
                  <Globe2 size={20} aria-hidden="true" />
                </span>
                <div>
                  <strong>外向きNAT</strong>
                  <p>Flash側の送信アクセス設定も適用されます。</p>
                </div>
                <Toggle
                  checked={spec.nat.enabled}
                  disabled={save.isPending}
                  onChange={({ detail }) =>
                    setSpec({
                      ...spec,
                      nat: { ...spec.nat, enabled: detail.checked },
                    })
                  }
                >
                  有効にする
                </Toggle>
              </div>
            </div>
            <div className="vpc-editor-section">
              <div className="vpc-editor-section__heading">
                <span>03</span>
                <div>
                  <h3>セキュリティグループ</h3>
                  <p>
                    通信相手をグループ化します。英小文字・数字・ハイフンを使用できます。
                  </p>
                </div>
              </div>
              <div
                className="vpc-group-list"
                aria-label="セキュリティグループ一覧"
              >
                {groups.map((group) => (
                  <div className="vpc-group-chip" key={group}>
                    <ShieldCheck size={15} aria-hidden="true" />
                    <span>{group}</span>
                    <button
                      type="button"
                      className="vpc-remove-icon"
                      aria-label={`${group}を削除`}
                      disabled={save.isPending || groups.length <= 1}
                      onClick={() => removeGroup(group)}
                    >
                      <Trash2 size={14} aria-hidden="true" />
                    </button>
                  </div>
                ))}
              </div>
              <div className="vpc-add-group">
                <FormField label="新しいセキュリティグループ">
                  <Input
                    value={newGroup}
                    placeholder="例: workspaces"
                    disabled={save.isPending}
                    onChange={({ detail }) => setNewGroup(detail.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        addGroup();
                      }
                    }}
                  />
                </FormField>
                <Button
                  formAction="none"
                  disabled={save.isPending || !newGroup.trim()}
                  onClick={addGroup}
                >
                  追加
                </Button>
              </div>
            </div>
            <div className="vpc-editor-section">
              <div className="vpc-editor-section__heading">
                <span>04</span>
                <div>
                  <h3>通信ルール</h3>
                  <p>
                    接続元・接続先と許可するポートを選びます。応答通信は自動で許可されます。
                  </p>
                </div>
              </div>
              <div className="vpc-rule-editor-list">
                {spec.rules.length === 0 && (
                  <div className="vpc-rule-empty">
                    ルールはまだありません。必要な接続を追加してください。
                  </div>
                )}
                {spec.rules.map((rule, index) => (
                  <div className="vpc-rule-editor" key={index}>
                    <div className="vpc-rule-editor__title">
                      <strong>ルール {index + 1}</strong>
                      <Button
                        formAction="none"
                        disabled={save.isPending}
                        onClick={() =>
                          setSpec({
                            ...spec,
                            rules: spec.rules.filter((_, i) => i !== index),
                          })
                        }
                      >
                        ルールを削除
                      </Button>
                    </div>
                    <SpaceBetween size="m">
                      <ColumnLayout columns={2}>
                        {(["source", "destination"] as const).map((key) => (
                          <FormField
                            key={key}
                            label={key === "source" ? "接続元" : "接続先"}
                          >
                            <Select
                              disabled={save.isPending}
                              selectedOption={
                                peerOptions.find(
                                  (o) => o.value === peerValue(rule[key]),
                                ) ?? {
                                  value: peerValue(rule[key]),
                                  label: peerValue(rule[key]),
                                }
                              }
                              options={peerOptions}
                              onChange={({ detail }) =>
                                changeRule(index, {
                                  [key]: parsePeer(
                                    detail.selectedOption.value ?? "all",
                                  ),
                                })
                              }
                            />
                          </FormField>
                        ))}
                      </ColumnLayout>
                      <ColumnLayout columns={3}>
                        <FormField label="プロトコル">
                          <Select
                            disabled={save.isPending}
                            options={[
                              { value: "tcp", label: "TCP" },
                              { value: "udp", label: "UDP" },
                            ]}
                            selectedOption={{
                              value: rule.protocol,
                              label: rule.protocol.toUpperCase(),
                            }}
                            onChange={({ detail }) =>
                              changeRule(index, {
                                protocol:
                                  detail.selectedOption.value === "udp"
                                    ? "udp"
                                    : "tcp",
                              })
                            }
                          />
                        </FormField>
                        <FormField label="開始ポート">
                          <Input
                            type="number"
                            value={String(rule.port)}
                            disabled={save.isPending}
                            onChange={({ detail }) =>
                              changeRule(index, { port: Number(detail.value) })
                            }
                          />
                        </FormField>
                        <FormField label="終了ポート（省略可）">
                          <Input
                            type="number"
                            value={
                              rule.end_port === undefined
                                ? ""
                                : String(rule.end_port)
                            }
                            disabled={save.isPending}
                            onChange={({ detail }) =>
                              changeRule(index, {
                                end_port: detail.value
                                  ? Number(detail.value)
                                  : undefined,
                              })
                            }
                          />
                        </FormField>
                      </ColumnLayout>
                    </SpaceBetween>
                  </div>
                ))}
              </div>
              <Button
                formAction="none"
                iconName="add-plus"
                disabled={save.isPending || spec.rules.length >= 128}
                onClick={() =>
                  setSpec({
                    ...spec,
                    rules: [
                      ...spec.rules,
                      {
                        description: "",
                        source: parsePeer(peerOptions[0]?.value ?? "all"),
                        destination: parsePeer(peerOptions[0]?.value ?? "all"),
                        protocol: "tcp",
                        port: 443,
                      },
                    ],
                  })
                }
              >
                ルールを追加
              </Button>
              <p className="vpc-muted">
                接続先サービスで宣言されているポートだけに適用されます。
              </p>
            </div>
            <div className="vpc-editor-actions">
              <Button formAction="none" onClick={() => setEdit(null)}>
                キャンセル
              </Button>
              <Button
                variant="primary"
                formAction="submit"
                loading={save.isPending}
              >
                保存
              </Button>
            </div>
          </SpaceBetween>
        </form>
      </Modal>
      <Modal
        visible={deleting !== null}
        onDismiss={() => setDeleting(null)}
        header="VPCを削除"
      >
        <SpaceBetween size="m">
          <Box>{deleting?.name} を削除します。</Box>
          {remove.isError && (
            <Alert type="error">{getApiErrorMessage(remove.error)}</Alert>
          )}
          <Button
            formAction="none"
            variant="primary"
            loading={remove.isPending}
            onClick={() => {
              if (deleting) remove.mutate(deleting.id);
            }}
          >
            削除する
          </Button>
        </SpaceBetween>
      </Modal>
    </SpaceBetween>
  );
}
