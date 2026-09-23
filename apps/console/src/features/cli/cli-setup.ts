const RELEASES_URL =
  "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/latest";

export interface CliSetupTarget {
  endpoint: string;
  organizationId: string;
}

function normalizedOrigin(endpoint: string): string {
  return new URL(endpoint).origin;
}

export function buildCliSetupPrompt({
  endpoint,
  organizationId,
}: CliSetupTarget): string {
  const origin = normalizedOrigin(endpoint);
  const insecureHttp = origin.startsWith("http://");

  return `このコンピューターへ HeteroCloud CLI を導入し、初期設定まで完了してください。説明だけで終わらず、可能な作業は端末上で実行してください。

接続先:
- HeteroCloud endpoint: ${origin}
- Organization ID: ${organizationId}
- 公式リリース: ${RELEASES_URL}

次の条件をすべて守ってください。
1. OS、CPUアーキテクチャ、利用中のシェルを検出し、最新リリースから一致するアーカイブを選んでください。対応対象は Linux/macOS/Windows の x64/ARM64 です。
2. アーカイブと同名の .sha256 をダウンロードし、SHA-256 が一致した場合だけ展開してください。
3. heterocloud（Windowsでは heterocloud.exe）を PATH 上へ配置してください。ユーザー領域で導入できない場合だけ、理由を説明して通常のOS権限昇格を求めてください。
4. HETEROCLOUD_ENDPOINT=${origin} と HETEROCLOUD_ORGANIZATION_ID=${organizationId} を、現在のユーザーとシェルで次回起動後も有効になるよう設定してください。
5. APIキーをチャット、プロンプト、URL、コマンドライン引数、シェル履歴、ログへ出さないでください。既存の安全なキーファイルがあれば HETEROCLOUD_API_KEY_FILE に設定してください。なければ、端末上の非表示入力でキーを受け取り、ユーザー専用の設定ディレクトリへ保存する短いローカル手順だけを用意してください。Unixではファイルを0600、Windowsでは現在ユーザーだけが読めるACLにしてください。APIキー自体を永続環境変数へ保存しないでください。
6. ${insecureHttp ? "接続先がHTTPなので、HETEROCLOUD_ALLOW_INSECURE_HTTP=1 も明示的に設定してください。" : "HETEROCLOUD_ALLOW_INSECURE_HTTP は設定しないでください。"}
7. 最後に heterocloud --version を実行し、実行ファイルの場所、バージョン、変更した設定ファイルと環境変数を簡潔に報告してください。サービスやクラウドリソースの作成・更新・削除は行わないでください。`;
}

export function buildChatGptLaunchUrl(prompt: string): string {
  const query = new URLSearchParams({ prompt });
  return `codex://threads/new?${query.toString()}`;
}
