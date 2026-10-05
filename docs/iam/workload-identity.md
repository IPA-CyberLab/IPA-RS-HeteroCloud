# FlashのタスクIAM

Flashサービスの `spec.task_role` に同じ組織の有効なサービスアカウントIDを指定すると、コンテナがそのアカウントのIAMポリシーでHeteroCloud APIを利用できます。個人用CLIトークンや固定APIキーの配布・月次更新は不要です。作成・編集画面の「タスクIAM」でも選択できます。既定は「なし」です。

## 管理者による設定

端末でデバイスコードログインを済ませたCLIから、次の操作を行います。組織とエンドポイントは通常のCLI設定を使います。

```sh
heterocloud iam service-accounts create --name coder-controller
heterocloud iam policies create --file policy.json
heterocloud iam bindings create --principal-id SERVICE_ACCOUNT_ID --policy-id POLICY_ID
```

例えば子サービスを作成・参照・更新・削除する専用アカウントには、以下のポリシーを設定します。`ORG_UUID` を実際の組織IDに置き換えてください。既知のサービスだけを扱う場合は、インスタンスのワイルドカードをそのIDに絞ります。

```json
{
  "name": "Coder workspaces",
  "document": {
    "version": "2026-07-31",
    "statements": [{
      "effect": "Allow",
      "actions": ["flash:CreateInstance", "flash:ListInstances", "flash:GetInstance", "flash:UpdateInstance", "flash:DeleteInstance"],
      "resources": ["hc:org:ORG_UUID:flash/*"]
    }]
  }
}
```

Flashの作成JSON・更新JSONの `spec.task_role` にアカウントIDを指定します。ロールを付ける呼び出し元には `iam:PassRole` と、通常のFlash作成・更新権限が必要です。`iam:PassRole` のリソースは `hc:org:ORG_UUID:iam/principal/SERVICE_ACCOUNT_ID` です。組織オーナーはこの操作ができます。サービスアカウントのAPIキー発行にも同じロール委譲権限を要求します。

親コンテナからVPC内の子を作る場合は、対象グループの `vpc:AttachSecurityGroup` も必要です。IAM権限とVPCの通信許可は別です。組織全体のFlash作成権限を与える例なので、ワークスペース専用の組織にするか、許可する操作を必要な範囲に絞ってください。親の権限・シークレット・タスクロールは子へ自動継承しません。

## コンテナからの利用

CLI 0.1.101以降をイメージへ導入すると、追加の `auth login` は不要です。

```sh
heterocloud iam whoami
heterocloud flash create --file child.json
heterocloud flash list
```

プロバイダーが `HETEROCLOUD_ENDPOINT`、`HETEROCLOUD_ORGANIZATION_ID`、`HETEROCLOUD_WORKLOAD_TOKEN_FILE` を注入します。エンドポイントは設置先の設定を使います。ドメイン名をイメージへ固定する必要はありません。これらの環境変数はタスクIAM有効時にはユーザー設定で上書きできません。明示的なAPIキーを設定した場合は、そのキーがCLIの認証で優先されるため、タスクIAMで使うコンテナには設定しないでください。

Podに紐づいたJWTはKubernetesが更新します。CLIはJWTをHTTPSの `POST /api/v1/auth/workload/token` に渡し、15分有効なAPIトークンを取得します。長時間実行するCLIプロセスは期限の60秒前から再取得し、その際は更新されたJWTファイルを読み直します。APIトークンはメモリ内だけに保持し、ログイン設定ファイルへ保存しません。別プロセスとして起動するCLIは毎回取得します。

独自クライアントは同じURLへフォーム形式で `grant_type=urn:ietf:params:oauth:grant-type:token-exchange`、`subject_token_type=urn:ietf:params:oauth:token-type:jwt`、`subject_token=<JWT>` をPOSTし、返されたBearerトークンを有効期限内に利用できます。トークンやJWTをコマンド引数・ログへ出さないでください。AWSのIMDSやAWS SDKとのプロトコル互換を提供する機能ではありません。

公開APIへ届く外向き通信が必要です。VPCではNATを有効にし、親サービスからAPIへの送信を許可してください。

## 権限と失効

暗黙の権限はなく、Allowがない操作と組織をまたぐ操作は拒否されます。明示的なDenyが優先されます。ポリシーはAPIリクエストごとに評価します。

```sh
heterocloud iam bindings list
heterocloud iam bindings delete BINDING_ID --yes
heterocloud iam service-accounts set-enabled SERVICE_ACCOUNT_ID --enabled false
heterocloud flash stop PARENT_ID
```

ポリシー解除は発行済みトークンにも反映されます。サービス停止・タスクロール変更・アカウント無効化では発行済みトークンを破棄し、再開・再割り当て・再有効化しても古いトークンは復活しません。APIはトークンのハッシュだけをDBへ保存します。停止したPodのJWTは交換時に拒否されます。Podの終了だけでは既に交換済みのAPIトークンは最大15分残るため、即時失効が必要ならサービス停止・ロール解除・アカウント無効化を使ってください。

IAM操作とロール使用は監査ログに記録します。認証情報の値は記録しません。CLIの `iam whoami` では組織・サービス・Pod・アカウントのIDを確認できます。

固定キーが必要な外部システム向けには、CLI OAuthログインから専用キーを発行・失効できます。発行結果は新規の専用ファイルへ保存し、標準出力へキーを表示しません。

```sh
heterocloud iam api-keys create SERVICE_ACCOUNT_ID --name integration --expires-in-days 30 --output-file ./integration-key.json
heterocloud iam api-keys revoke SERVICE_ACCOUNT_ID KEY_ID --yes
```

## 設置側

HeteroCloud Helmの `workloadIdentity.enabled` と `namespace`、Kubernetes API宛ての `apiCidrs` を設定します。Flash Helmの `workloadIdentity.endpoint` に公開APIのHTTPSオリジンを設定します。API側はTokenReviewの作成と指定ワークロードnamespaceのPod参照だけを許可され、ワークロードのサービスアカウントにはKubernetes API操作権限を与えません。
