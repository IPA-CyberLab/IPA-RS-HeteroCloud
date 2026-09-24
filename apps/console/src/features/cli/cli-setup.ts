const RELEASES_URL =
  "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/latest";
const SKILL_URL =
  "https://raw.githubusercontent.com/IPA-CyberLab/IPA-RS-HeteroCloud/d359be6fdf7b71d7a676df059fe9a5640d3e63f0/.agents/skills/heterocloud-cli-setup/SKILL.md";
const SKILL_SHA256 =
  "21df216663c8b55b6bd452a8efa5089c6e820625cfe7fbf08d43ee70a69e8091";

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
4. HeteroCloud CLIセットアップ用のClaude Code/Codex skillも、このユーザーへインストールしてください。公式リポジトリの固定コミットにある ${SKILL_URL} から SKILL.md を取得し、SHA-256が ${SKILL_SHA256} に一致した場合だけ配置してください。Claude Codeはユーザーの ~/.claude/skills/heterocloud-cli-setup/SKILL.md、Codexは CODEX_HOME があればその skills/heterocloud-cli-setup/SKILL.md、なければ ~/.codex/skills/heterocloud-cli-setup/SKILL.md です。Windowsでは ~ をユーザープロファイルに読み替えてください。両方の保存先を用意し、異なる既存ファイルがあればバックアップしてから更新し、保存後のファイルも同じSHA-256で検証してください。Claude CodeやCodex本体のインストールは不要です。
5. HETEROCLOUD_ENDPOINT=${origin} と HETEROCLOUD_ORGANIZATION_ID=${organizationId} を、現在のユーザーとシェルで次回起動後も有効になるよう設定してください。
6. ${insecureHttp ? "接続先がHTTPなので、HETEROCLOUD_ALLOW_INSECURE_HTTP=1 も明示的に設定してください。" : "HETEROCLOUD_ALLOW_INSECURE_HTTP は設定しないでください。"}
7. APIキーを要求・生成・表示しないでください。heterocloud auth login を実行し、表示された任意ドメイン上の認証URLを既定ブラウザで開き、ユーザーがKeycloakログインとCLI承認を完了できる状態にしてください。認証コードやアクセストークンをチャット、シェル履歴、URL、ログへ転記しないでください。
8. ブラウザ承認後にCLIが終了するまで待ち、heterocloud auth status を実行してください。ログイン済みユーザー、組織、トークン有効期限が取得でき、両skillの配置とSHA-256検証も成功した場合だけセットアップ完了としてください。
9. 最後に heterocloud --version を実行し、実行ファイルの場所、バージョン、変更した設定ファイルと環境変数、Claude Code/Codex skillの配置先、auth statusの成否を簡潔に報告してください。サービスやクラウドリソースの作成・更新・削除は行わないでください。`;
}

export function buildChatGptLaunchUrl(prompt: string): string {
  const query = new URLSearchParams({ prompt });
  return `codex://threads/new?${query.toString()}`;
}
