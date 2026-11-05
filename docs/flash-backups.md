# Flash の永続ディスクをバックアップ・復元する

コンソールの **Flash → サービス → サービス詳細 → 永続ディスクのバックアップ**
でバックアップ名を入力し、「バックアップを作成」を押します。
「復元可能」になったら「復元」を押し、復元先の名前とプロジェクトを指定します。
**Flash → バックアップ** では、元サービスを削除した後も一覧と復元を利用できます。

復元では **新しいディスクと停止状態の非公開サービス** を作ります。
元サービスと元ディスクの内容は変更しません。復元が完了してからサービスを編集し、
必要な環境変数、シークレット接続、GPU、ネットワーク、タスクロールを設定して起動してください。
外部への送信も初期状態では無効です。元の公開ドメインを自動で切り替えることはありません。

## 保存対象と整合性

- 永続ディスクにある `/root` の内容を保存します。
- 一時ディスク、書き込みレイヤー、環境変数、シークレット接続、追加マウントは対象外です。
- 稼働中も保存できますが、クラッシュ整合性です。データベースなどの書き込みをアプリケーションで
  停止・同期するか、Flash サービスを停止してから保存してください。
- 「受付日時」は API が依頼を受け付けた時刻です。保存は非同期で実行し、転送完了を確認した
  バックアップだけが「復元可能」になります。保存先を確認できない場合は最終取得状態と表示します。
- 同じ組織内でのみ復元できます。別ユーザーへの共有、別組織へのコピーは含みません。

サービスを削除してもバックアップは残ります。バックアップを削除しても、元ディスクと
復元済みのディスクは残ります。復元中のバックアップ削除は、ディスク作成が終わるまで待機します。

## 定期バックアップ

サービス詳細の「運用」から「定期バックアップ」を選び、有効化、取得間隔、保存世代数、
復元検証を設定できます。取得間隔は1〜168時間、保存世代数は1〜8です。手動バックアップは
定期バックアップの世代整理には含まれません。新しいバックアップの保存と、指定した場合の
復元検証が完了してから、古い世代を整理します。

CLI では JSON を標準入力から渡せます。以下は24時間ごとに取得し、2世代保持する例です。

```sh
printf '%s\n' '{"enabled":true,"interval_seconds":86400,"retention_count":2,"verify_restore":true}' | \
  heterocloud flash backup-policy set SERVICE_ID --file -
heterocloud flash backup-policy get SERVICE_ID
```

定期処理は、設定者のバックアップ作成・削除権限と、復元検証を有効にした場合は復元権限を
実行時にも確認します。定期保存も手動バックアップと同じ容量上限を使用します。

## 保存先

既定は運営が管理する Syouyu です。組織の管理者は **Flash → バックアップ** の
「S3の保存先を追加」から、外部の S3 または S3 互換ストレージを登録できます。
手動バックアップと定期バックアップの設定では、それぞれ保存先を選択します。
保存先の切り替えは新しいバックアップにだけ適用され、以前のバックアップは作成時の保存先から復元します。
保存先ごとに定期バックアップの世代を管理します。

外部保存先には公開 HTTPS エンドポイント、バケット、リージョン、アクセスキーと
シークレットキーが必要です。パス形式または仮想ホスト形式と、任意のプレフィックスを指定できます。
運営が管理する Syouyu は内部接続を使用しますが、利用者が登録する内部アドレスやプライベートな
S3 エンドポイントには対応していません。セッショントークン付き一時資格情報の自動更新にも対応していません。
アクセスキーは指定バケットのみに必要な読み書き・一覧・削除権限を付与してください。
バックアップは指定プレフィックスの `heterocloud/組織ID/保存先ID/` 以下に保存します。
認証情報はシークレットとして保存され、一覧や設定画面からは再表示されません。
切れた資格情報は保存先一覧の「認証情報を更新」から入れ替えます。

## CLI

CLI 0.1.106 以降で利用できます。接続先は既存の `HETEROCLOUD_ENDPOINT`、
組織は `HETEROCLOUD_ORGANIZATION_ID` または通常の CLI オプションで選択します。
外部の保存先登録と `--destination-id` は CLI 0.1.112 以降が必要です。

```sh
heterocloud update
heterocloud flash backups create SERVICE_ID --name before-upgrade
heterocloud flash backup-destinations list
heterocloud flash backup-destinations create --file s3-destination.json
heterocloud flash backups create SERVICE_ID --name offsite --destination-id DESTINATION_ID
heterocloud flash backups list
heterocloud flash backups list --service-id SERVICE_ID
heterocloud flash backups get BACKUP_ID
heterocloud flash backups restore BACKUP_ID --project-id PROJECT_ID --name recovered
heterocloud flash backups delete BACKUP_ID --yes
```

作成・復元・削除は既定で完了まで待ちます。受付直後に戻る場合は `--no-wait` を使ってください。
ネットワーク切断後の再送には、作成・復元時に表示された `--request-id UUID` を再利用します。
同じリクエスト ID で内容を変えた再送は拒否します。
保存先登録用の JSON は、`name`, `endpoint`, `bucket`, `region`, `access_key`, `secret_key` と、
省略可能な `prefix`, `virtual_hosted`, `request_id` を指定します。ファイルは秘密情報なので
Unix 系では `chmod 600` にしてください。標準入力を使う場合は `--file -` を指定できます。
資格情報の更新は、`access_key` と `secret_key` だけを含む同様の JSON を
`heterocloud flash backup-destinations rotate-credentials DESTINATION_ID --file -` へ渡します。

## 権限と API

`contracts/api/v1/flash.openapi.json` にリクエスト・レスポンスを定義しています。

| 操作 | IAM アクション | リソース |
| --- | --- | --- |
| 作成 | `flash:CreateBackup` | `hc:org:ORG:flash/instance/SERVICE_ID` |
| サービス単位の一覧 | `flash:ListBackups` | 同上 |
| 組織全体の一覧 | `flash:ListBackups` | `hc:org:ORG:flash/backup/*` |
| 取得・削除・復元 | `flash:GetBackup` / `flash:DeleteBackup` / `flash:RestoreBackup` | `hc:org:ORG:flash/backup/BACKUP_ID` |
| 保存先の登録・資格情報の更新 | `flash:ManageBackupDestinations` | `hc:org:ORG:flash/backup-destination/*` |

復元には通常の `flash:CreateInstance` 権限と、復元先サービスのリソース枠も必要です。
組織所有者の既存の全権限ポリシーから利用できます。Cookie 認証の変更操作には通常の CSRF 保護を適用します。

## 保存量と運用

初期制限はサービスごとに 10 個、組織ごとに 100 個、対象ディスク容量の合計 128 GiB です。
失敗・削除処理中の依頼も、保存先の削除を確認するまで枠を予約します。
「対象ディスク容量」は論理容量で、圧縮・重複排除後の保存量や請求額ではありません。

本番では Longhorn CSI の `type: bak` を使い、選択した保存先に保存します。
利用者が登録する S3 のキーを特定のバケットに限定することは、登録者側で設定してください。
CSI スナップショットのメタデータも復元に必要なので、Kubernetes/etcd の運用バックアップも保持してください。
現在の保存先は同一クラスターの Syouyu で、物理保存量に全体 40 GiB の上限を設けています。
元サービスや元ボリュームの削除には耐えますが、同一クラスターの Syouyu だけでは
**クラスターや拠点全体の消失に対する復旧は保証しません**。外部 S3 を使う場合でも、
CSI メタデータ、シークレット、コントロールプレーンのデータベースを別途バックアップして
復旧を検証する必要があります。
