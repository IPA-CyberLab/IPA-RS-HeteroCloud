# 組織横断の権限セット

管理アカウントの管理者が権限セットを作り、組織内のアカウントにユーザーまたはグループを割り当てる。割り当てを受けたユーザーは、自分の IdP セッションから対象アカウントの短期ロールへ切り替える。アカウントごとにプロビジョニングしたロールには権限セットのポリシーと対象アカウントの SCP の両方を適用する。

安全性の要件:

- 各ロールとポリシーの作成・割り当ては単一の DB トランザクションで完了する。
- ロールの通常の AssumeRole、IAM によるポリシー追加・削除や信頼ポリシー変更は拒否する。権限セットの管理 API からだけ更新する。
- セッション発行時および各操作時にユーザー、割り当て、グループ所属を検証する。割り当て削除やグループ退会を取り消しても古いセッションは復活させない。
- ブラウザの切り替え失敗時や失効時に元の強い権限へ暗黙に戻らない。
- 全 API が新しいセッション検証を理解するまで DB の有効化フラグを false に保つ。ロール・ポリシーなどの古い操作経路も同じフラグの前に保護し、混在中にセッションを発行しない。
- 権限セットの設定およびユーザーとグループの割り当ては管理アカウントで認可・監査する。対象アカウントの SCP が明示的に拒否する操作は許可しない。

HeteroCloud の既存 IdP アカウントを使用し、初期段階では管理アカウントのメンバーをディレクトリ利用者とする。AWS の IAM Identity Center API、SCIM、AWS CLI/SDK の SSO トークン形式との互換性を暗黙に主張しない。AWS のモデルは [permission sets](https://docs.aws.amazon.com/singlesignon/latest/userguide/permissionsetsconcept.html) と [managed IAM roles](https://docs.aws.amazon.com/singlesignon/latest/userguide/identity-center-and-iam-roles.html) を参照する。
