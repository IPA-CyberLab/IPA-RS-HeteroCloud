# VPCとFlashのプライベート接続

VPCは同じ組織・プロジェクト・リージョンのFlashサービスを接続します。内部通信は既定で拒否され、接続元・接続先のサービスまたはセキュリティグループとTCP / UDPポートで許可します。

```sh
heterocloud vpc create --file vpc.json
heterocloud vpc list
heterocloud vpc get VPC_ID
heterocloud vpc update VPC_ID --file vpc-update.json
heterocloud flash create --file child.json
```

`examples/cli/vpc.json` のプロジェクトIDとリージョンを置き換えてください。Flashの `spec.network` に `vpc_id`、`security_groups`、任意の `private_name` を指定します。外部公開しないサービスには `exposure.type: internal` を指定してください。

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

NATは既定で無効です。VPCの `spec.nat.enabled` を有効にすると、Flashの `egress.mode` と許可・拒否CIDRに従って公開IPv4宛先へ接続できます。NATを有効にしても外部からの接続は許可されません。ゲートウェイ障害時は通常のノード経路への迂回を拒否します。送信元IPはゲートウェイノードの出口IPで、切替時に変わる場合があります。

Coderなどの親コンテナは、専用サービスアカウントでHeteroCloud API / CLIから子を作成します。必要なFlash操作と、子を接続するグループの `vpc:AttachSecurityGroup` だけをIAMで許可してください。APIキーは親のSecret Managerに登録し、Flash編集画面から環境変数として接続します。CLIは `HETEROCLOUD_API_KEY_FILE`（アクセス権0600のファイル）を使えます。キーをコマンド引数やイメージへ埋め込まないでください。

VPCのAPIは `/api/v1/organizations/{organization_id}/vpc/networks` のPOST / GETと、その `/{id}` のGET / PUT / DELETEです。IAMリソースは `hc:org:<org>:vpc/network/<id>`、グループ接続は末尾 `/security-group/<group>` です。VPCとFlashの変更は非同期に反映され、CLIは既定でreadyまで待ちます。接続先の組織・プロジェクト・リージョンが異なる場合や、使用中のVPCの削除は拒否されます。

サービスアカウントのFlash作成権限は組織単位です。秘密情報は子へ自動継承されません。接続ルール削除は新しい接続に適用されます。確立済みの接続はステートフルに扱われます。Podアドレスを変えずに分離する方式であり、独自・重複CIDRやIPv6 NATの指定はありません。

運用構成とデータプレーンの詳細: [HeteroCloud VPC](https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-VPC)。
