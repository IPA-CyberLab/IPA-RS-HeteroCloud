# アカウントアクセス

管理アカウントの管理者は、権限セットを作成し、組織内のアカウントにユーザーまたはグループを割り当てられます。ユーザーとグループには管理アカウントのメンバーを利用します。割り当てられた人はコンソールの「アカウントアクセス」から権限セットを選び、対象アカウントに切り替えます。

権限セットごとに対象アカウント内の管理対象ロールが作成され、権限セットと SCP の両方が適用されます。グループからの削除や割り当ての解除はセッションにも反映され、後から再追加しても古いセッションは復活しません。割り当て先に個人上限のためのメンバーが自動作成され、作成されたリソースは利用者に計上されます。割り当て解除後もメンバーとリソースは消しません。

## CLI

管理アカウントにデバイスコードでログインした後、利用可能な割り当てを一覧し、一時セッションを安全なファイルに保存します。

```sh
heterocloud iam identity-center my-assignments
heterocloud iam identity-center start-session ASSIGNMENT_ID --output-file ./role-session.token
```

コマンドは対象の `account_id` と有効期限を表示します。次のコマンドでは対象アカウントを指定し、保存したファイルを使います。エンドポイントは設置先の値を設定してください。

```sh
HETEROCLOUD_API_KEY_FILE=./role-session.token \
HETEROCLOUD_ORGANIZATION_ID=TARGET_ACCOUNT_ID \
heterocloud flash list
```

ファイルは新規作成のみで、Unix では所有者以外から読めない必要があります。トークンを表示・共有せず、有効期限後は削除してください。管理者向けには `users`、`groups`、`permission-sets`、`assignments` の各サブコマンドがあります。JSON ファイルは対応 API と同じフィールドを指定し、CLI の `--help` で確認できます。

現在は HeteroCloud の OIDC ログインと CLI を使用します。AWS IAM Identity Center、SCIM、AWS CLI/SDK の SSO トークン形式とは互換ではありません。アカウントアクセスのセッションから別の IAM ロールへのチェーンは、利用者ごとの上限と失効を維持できるまで拒否します。
