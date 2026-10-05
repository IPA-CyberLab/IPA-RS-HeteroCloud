# Flashのロードバランサー認証

HTTP/HTTPS向けの `web` エンドポイントで、サービスごとに「認証なし」または「OIDC」を選べます。既定は認証なしで、既存サービスの動作は変わりません。コンソールの作成・編集画面、API、CLIから設定できます。TCP/UDPのIP・ロードバランサーモードにはブラウザ向けOIDCを適用しません。

## 設定

作成・更新JSONの `spec.exposure` に次を指定します。IssuerはHTTPSで提供される実際のOIDCプロバイダーのURLです。

```json
{
  "type": "public",
  "traffic_mode": "forwarded",
  "endpoint_mode": "web",
  "authentication": {
    "issuer_url": "https://identity.example.org/realms/apps",
    "client_id": "my-web-service",
    "client_secret_ref": "oidc-client-secret",
    "scopes": ["openid", "profile", "email"]
  }
}
```

ポートはHTTPを待ち受けるTCPポート1つです。コンテナにTLS終端やOIDCライブラリを組み込む必要はありません。公開URLはFlashのステータスから取得します。`oidc_callback_url` に返されるURLをIdPの許可リダイレクトURIへ登録してください。設定前のwebサービスでもこのフィールドを返します。認証設定中のサービスでは、準備が完了する前もAPIから現在のコールバックURLを取得できます。

クライアントシークレットは別の書き込みAPIで保存し、スペックには参照名だけを保存します。CLIでは末尾改行を入れず、秘密値をファイルか標準入力から渡してください。

```sh
heterocloud flash load-balancer-secret set SERVICE_ID oidc-client-secret --file ./client-secret
heterocloud flash update SERVICE_ID --file service-update.json
```

APIでは `PUT /api/v1/organizations/ORG_ID/flash/services/SERVICE_ID/load-balancer/secrets/NAME` に `{"value":"..."}` を送ります。コンソールのパスワード欄を空のまま編集すると、保存済みの値を維持します。秘密値は一覧・サービス仕様・コンテナの環境変数に出しません。Secret Managerを保管先とし、GatewayのためにKubernetes Secretにも必要なコピーを置きます。Kubernetes Secretを紛失した場合は同じAPIから再保存してください。

有効化・更新中に認証ポリシーが未受理の場合やシークレットがない場合、公開ルートはアプリケーションへ転送しません。IdP障害時も認証を迂回しません。設定は非同期なので、`ready` とログイン動作を確認してから利用してください。ブラウザはIdPへ移動し、成功後にアクセス元のURLへ戻ります。認証用Cookieは暗号化しサービスのホスト内に限定します。トークンをアプリケーションへ転送しません。この機能はOIDCログインを要求するもので、IdPのユーザー・グループによる追加認可はアプリケーション側で設定してください。

ログアウトURLは公開サービスの `/_heterocloud/oidc/logout` です。これはロードバランサーのセッションを解除します。IdP自体のセッションの終了はIdP側で扱います。

## 解除・変更

`spec.exposure.authentication` を削除または `null` にして更新すると、認証なしになります。OIDCの設定・クライアントシークレットの変更だけではFlashコンテナを再起動しません。

使用中のシークレットは削除を拒否します。参照を外してから、以下で削除できます。

```sh
heterocloud flash load-balancer-secret delete SERVICE_ID oidc-client-secret --yes
```

クライアントシークレットを更新するとロードバランサーの認証Cookieも更新するため、利用者は再ログインします。公開URLやIdPのドメインは設置先・サービスの設定で決まり、CLIやイメージへ固定する必要はありません。

設置にはEnvoy Gateway 1.8.4以降を使用してください。HTTPRouteとSecurityPolicy、OIDCシークレットの管理権限がFlashコントローラーに必要です。IdPへのHTTPSアクセスと名前解決をGatewayのネットワークで許可してください。
