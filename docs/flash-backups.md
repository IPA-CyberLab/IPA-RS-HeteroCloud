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
- 同じ組織内でのみ復元できます。別ユーザーへの共有、別組織へのコピー、自動定期保存は含みません。

サービスを削除してもバックアップは残ります。バックアップを削除しても、元ディスクと
復元済みのディスクは残ります。復元中のバックアップ削除は、ディスク作成が終わるまで待機します。

## CLI

CLI 0.1.106 以降で利用できます。接続先は既存の `HETEROCLOUD_ENDPOINT`、
組織は `HETEROCLOUD_ORGANIZATION_ID` または通常の CLI オプションで選択します。

```sh
heterocloud update
heterocloud flash backups create SERVICE_ID --name before-upgrade
heterocloud flash backups list
heterocloud flash backups list --service-id SERVICE_ID
heterocloud flash backups get BACKUP_ID
heterocloud flash backups restore BACKUP_ID --project-id PROJECT_ID --name recovered
heterocloud flash backups delete BACKUP_ID --yes
```

作成・復元・削除は既定で完了まで待ちます。受付直後に戻る場合は `--no-wait` を使ってください。
ネットワーク切断後の再送には、作成・復元時に表示された `--request-id UUID` を再利用します。
同じリクエスト ID で内容を変えた再送は拒否します。

## 権限と API

`contracts/api/v1/flash.openapi.json` にリクエスト・レスポンスを定義しています。

| 操作 | IAM アクション | リソース |
| --- | --- | --- |
| 作成 | `flash:CreateBackup` | `hc:org:ORG:flash/instance/SERVICE_ID` |
| サービス単位の一覧 | `flash:ListBackups` | 同上 |
| 組織全体の一覧 | `flash:ListBackups` | `hc:org:ORG:flash/backup/*` |
| 取得・削除・復元 | `flash:GetBackup` / `flash:DeleteBackup` / `flash:RestoreBackup` | `hc:org:ORG:flash/backup/BACKUP_ID` |

復元には通常の `flash:CreateInstance` 権限と、復元先サービスのリソース枠も必要です。
組織所有者の既存の全権限ポリシーから利用できます。Cookie 認証の変更操作には通常の CSRF 保護を適用します。

## 保存量と運用

初期制限はサービスごとに 10 個、組織ごとに 100 個、対象ディスク容量の合計 128 GiB です。
失敗・削除処理中の依頼も、保存先の削除を確認するまで枠を予約します。
「対象ディスク容量」は論理容量で、圧縮・重複排除後の保存量や請求額ではありません。

本番では Longhorn CSI の `type: bak` を使い、専用の非公開 S3 バケットへ保存します。
キーはそのバケットだけの読み書きに限定し、利用者 API には出力しません。
CSI スナップショットのメタデータも復元に必要なので、Kubernetes/etcd の運用バックアップも保持してください。
現在の保存先は同一クラスターの Syouyu で、物理保存量に全体 40 GiB の上限を設けています。
元サービスや元ボリュームの削除には耐えますが、**クラスターや拠点全体の消失に対する復旧は保証しません**。
その要件には別拠点の S3 保存先とメタデータの災害復旧手順が必要です。
