# VPCとFlashのプライベート接続

VPCは同じ組織・プロジェクト・リージョンのFlashサービスを接続します。内部通信は既定で拒否され、接続元・接続先のサービスまたはセキュリティグループとTCP / UDPポートで許可します。

```sh
heterocloud vpc create --file vpc.json
heterocloud vpc list
heterocloud vpc get VPC_ID
heterocloud vpc update VPC_ID --file vpc-update.json
heterocloud flash create --file child.json
```

`examples/cli/vpc.json` のプロジェクトIDとリージョンを置き換えてください。子の作成には `examples/cli/flash-vpc-private.json` のプロジェクトID、リージョン、イメージ、作成したVPCのIDを置き換え、`heterocloud flash create --file examples/cli/flash-vpc-private.json` を実行します。Flashの `spec.network` に `vpc_id`、`security_groups`、任意の `private_name` を指定します。外部公開しないサービスには `exposure.type: internal` を指定してください。

```json
{
  "network": {
    "vpc_id": "VPC_UUID",
    "security_groups": ["workspaces"],
    "private_name": "workspace-one"
  },
  "exposure": {"type": "internal", "traffic_mode": "forwarded"},
  "ports": [{"name": "ssh", "protocol": "tcp", "container_port": 22}]
}
```

内部接続先はFlashの `status.private_endpoints`（保存済みの応答では `status.status.private_endpoints`）とコンソールの「VPCのプライベート接続」に表示されます。ポートはコンテナポートです。内部向けサービスは公開ポート枠を消費しません。

## 選んだサービスをインターネットへ公開する

VPCに接続したまま、Flashサービス単位で公開できます。例えばCoderホストだけを公開し、そのホストが作るワークスペースは内部向けにできます。公開と内部通信の許可は独立しており、公開しただけでは他のVPCサービスからの直接接続は許可されません。

| 通信の向き | 設定 |
| --- | --- |
| インターネット → 選んだFlash | Flashの `exposure.type: public` |
| Flash → インターネット | VPCの `nat.enabled: true` とFlashの `egress.mode: internet` または `restricted` |
| Flash → VPC内のFlash | VPCの接続元・接続先・ポートのルール |

HTTP/HTTPSで公開する例は [`examples/cli/flash-vpc-public.json`](../../examples/cli/flash-vpc-public.json) です。プロジェクトID、リージョン、VPC IDを置き換えて実行します。グループ `coder` は同じVPCに作成しておいてください。APIでは同じJSONを `/api/v1/organizations/{organization_id}/flash/services` へPOSTします。

```sh
heterocloud flash create --file examples/cli/flash-vpc-public.json
heterocloud flash get SERVICE_ID
```

公開URLは返されたFlashの `status.endpoints`（保存済みの応答では `status.status.endpoints`）を使います。ホスト名はデプロイ先の設定で決まるため、アプリケーションに固定のドメインを埋め込む必要はありません。VPC内では引き続き `private_endpoints` のDNS名で接続できます。この例は外向き通信を無効にしており、NATが無効でも公開URLへの受信とその応答は動作します。CoderホストからAPIへ接続する場合は、下記のNATも有効にしてください。

HTTPS以外のTCP / UDPを公開する場合は `exposure.endpoint_mode: ip` または `load_balancer` を指定し、`ports` に各プロトコルとコンテナポートを設定します。接続先のホストと割り当てポートは `status.endpoints` から取得してください。これらのモードでは `exposure.allowed_source_cidrs` / `denied_source_cidrs` で受信元を制限できます。HTTP/HTTPSの `web` モードはTCPポート1つのみで、受信元CIDRの指定には未対応です。

公開を解除するには現在の設定を取得し、`spec.exposure` を `{"type":"internal","traffic_mode":"forwarded","endpoint_mode":"ip"}` に変更して更新します。`spec.network` とVPCルールを維持すれば、許可した内部通信は継続します。公開解除は非同期なので、CLIの完了後に外部URLが応答しなくなったことも確認してください。

```sh
heterocloud flash get SERVICE_ID > service.json
jq '{name, spec: (.spec | .exposure = {type:"internal", traffic_mode:"forwarded", endpoint_mode:"ip"} | if .autoscaling then .autoscaling.min_replicas = ([.autoscaling.min_replicas, 1] | max) else . end)}' service.json > private-update.json
heterocloud flash update SERVICE_ID --file private-update.json
```

内部向けサービスでは最小レプリカ数は1以上です。既に確立している接続と、新しい接続への公開停止は別に扱われます。

## インターネットへの外向き通信（NAT）

NATは既定で無効です。VPCの `spec.nat.enabled` を有効にすると、Flashの `egress.mode` と許可・拒否CIDRに従って公開IPv4宛先へ接続できます。NATを有効にしても外部からの接続は許可されません。ゲートウェイ障害時は通常のノード経路への迂回を拒否します。送信元IPはゲートウェイノードの出口IPで、切替時に変わる場合があります。

## 親から子を作成する

Coderなどの親コンテナには、専用サービスアカウントを `spec.task_role` に割り当てます。CLI 0.1.98以降はコンテナのPod IDから短時間の認証情報を自動取得・更新するため、固定APIキーや個人のCLIログイントークンを渡す必要はありません。必要なFlash操作と、子を接続するグループの `vpc:AttachSecurityGroup` だけをIAMで許可してください。ロールを割り当てる呼び出し元には `iam:PassRole` も必要です。設定例と失効操作は [タスクIAM](../iam/workload-identity.md) を参照してください。

親から公開HeteroCloud APIへアクセスする場合は、例のVPCの `spec.nat.enabled` を `true` にし、親の `spec.egress.mode` を `internet` にします。NATを有効にしても、内部向けに作成した子への外部公開は有効になりません。子の外向き通信が不要なら、子の `spec.egress.mode` は `disabled` にできます。

VPCのAPIは `/api/v1/organizations/{organization_id}/vpc/networks` のPOST / GETと、その `/{id}` のGET / PUT / DELETEです。IAMリソースは `hc:org:<org>:vpc/network/<id>`、グループ接続は末尾 `/security-group/<group>` です。VPCとFlashの変更は非同期に反映され、CLIは既定でreadyまで待ちます。接続先の組織・プロジェクト・リージョンが異なる場合や、使用中のVPCの削除は拒否されます。

サービスアカウントのFlash作成権限は組織単位です。秘密情報は子へ自動継承されません。接続ルール削除は新しい接続に適用されます。確立済みの接続はステートフルに扱われます。Podアドレスを変えずに分離する方式であり、独自・重複CIDRやIPv6 NATの指定はありません。

運用構成とデータプレーンの詳細: [HeteroCloud VPC](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-VPC)。

実環境での検証結果: [2026-10-03のVPC / Flash E2E](../verification/vpc-2026-10-03.md)。

外部公開を含む追加検証: [2026-10-04の検証結果](../verification/vpc-external-2026-10-04.md)。通信検証は通過しましたが、削除確認APIの503によりE2E全体は未合格です。
