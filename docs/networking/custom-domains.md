# Flashの独自ドメイン

公開HTTP/HTTPSのFlashサービスに、自分のドメインを追加できます。通常のFlash URLも引き続き利用できます。コンテナを再起動する必要はありません。

## 設定

サービス詳細の「編集」→「独自ドメイン」で `app.example.com` を登録します。表示された接続先（`f-サービスID.設定済みのFlashドメイン`）へ **CNAME** を設定してください。DNSプロバイダーのプロキシはオフ（DNSのみ）にします。

DNSを確認するとHTTPS証明書が自動で発行され、「利用可能」になります。証明書は自動更新されます。DNS反映と証明書発行には数分かかる場合があります。URL・ワイルドカード・IPアドレス・内部ドメインは登録できません。

ルートドメインなどCNAMEを置けない名前では、表示された `_heterocloud.ドメイン` のTXTと、接続先に追従するALIAS／ANAME／CNAME flatteningを使用できます。A／AAAAを手動指定する場合もTXTが必要で、公開Flash接続先のIPに一致している必要があります。固定Aレコードは入口の切り替えに追従しないため、ALIASなどを推奨します。

## CLI

```sh
heterocloud update
heterocloud flash domains add SERVICE_ID --hostname app.example.com
heterocloud flash domains list SERVICE_ID
heterocloud flash domains delete SERVICE_ID DOMAIN_ID --yes
```

CLIの設定済みエンドポイント・組織を使用します。1サービス8ドメイン、現在の構成ではシステム全体128ドメインまで登録できます。

## APIと権限

APIパスは `/api/v1/organizations/ORG/flash/services/SERVICE/domains` です。

- `GET`: 一覧・CNAME接続先・TXT確認値・証明書状態。`flash:GetInstance` が必要。
- `POST {"hostname":"app.example.com"}`: 登録しHTTP 202を返す。`flash:UpdateInstance` が必要。
- `DELETE .../domains/DOMAIN_ID`: 削除を受け付けHTTP 202を返す。同じ更新権限が必要。

ホスト名はシステム全体で一意です。削除中は予約を保持し、入口・ルート・証明書の削除後に解放します。プロバイダーが一時停止しても登録情報は保持し、復旧後に再適用します。

## OIDC認証

サービスに設定したロードバランサー認証は独自ドメインにも適用されます。認証プロバイダーの許可コールバックに、一覧に表示される `https://独自ドメイン/_heterocloud/oidc/callback` を追加してください。認証設定の反映中はアクセスを遮断します。認証Cookieはそのホストだけに適用します。

## 運用

TerraformでArgo CDの `custom-domains` アプリケーションとcert-managerのGateway API設定を管理します。独立した証明書名前空間、専用HTTP-01ゲートウェイ、2台の確認コントローラー、公開入口のTLS同期を使います。HTTP-01には独自ドメインの公開TCP 80が必要です。

コントローラーはDNSプロバイダー資格情報やユーザーのシークレットを読みません。証明書と秘密鍵は専用名前空間で管理し、公開入口には名前を限定したSecretだけを投影します。登録・削除・更新はFlashのPod設定に影響しません。
