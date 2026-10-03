import type { SitePage } from "./types.ts";

export const productPages: SitePage[] = [
  {
    path: "/",
    title: "HeteroCloud — 分散したリソースを、ひとつのクラウドへ",
    description: "HeteroCloudは、コンテナ、リアルタイム通信、ストレージを組織とプロジェクト単位で管理するクラウドプラットフォームです。HeteroNetが、そのネットワークを支えます。",
    lead: "アプリケーションを動かす。端末をつなぐ。データを保存する。HeteroCloudは、分散した計算資源とネットワークを使うためのクラウドプラットフォームです。",
    sections: [
      { id: "cloud", title: "HeteroCloudでできること", paragraphs: ["コンテナの実行、リアルタイム通信、オブジェクトストレージを、共通のコンソールとCLIから管理します。組織・プロジェクト・アクセス権限・利用量をまとめて扱えるため、アプリケーションごとに必要なサービスを組み合わせられます。"], links: [{ label: "HeteroCloudのサービスを知る", href: "/heterocloud/" }, { label: "コンソールへ", href: "/login" }] },
      { id: "services", title: "サービス", table: { headings: ["サービス", "用途"], rows: [
        ["Flash / Flash Registry", "コンテナの実行と、実行するイメージの管理。"],
        ["Flow", "ルーム、シグナリング、STUN/TURNを使ったリアルタイム通信。"],
        ["Syouyu", "S3互換APIで扱うオブジェクトストレージ。"],
        ["Secret Manager", "アプリケーションが使うシークレットの管理と、コンテナへの環境変数としての接続。"],
      ] } },
      { id: "network", title: "ネットワークを支えるHeteroNet", paragraphs: ["HeteroNet（HeteroNetwork）は、異なる場所やネットワークにあるマシンをつなぐP2P VPNです。接続できる相手とは直接通信し、直接接続が難しい場合は中継経路を利用します。HeteroCloudの基盤ネットワークとしても使われています。"], links: [{ label: "HeteroNetの仕組みを見る", href: "/heteronet/" }] },
      { id: "understand", title: "使い方と仕組みを公開", paragraphs: ["まずサービスの役割を知り、利用ガイドからコンソールへ進めます。実装に関心がある方には、制御プレーンとデータプレーン、レイジー接続、コンテナの分離などの技術解説を用意しています。"], links: [{ label: "利用ガイド", href: "/getting-started/" }, { label: "技術解説", href: "/technology/" }, { label: "規約・ポリシー", href: "/legal/" }] },
    ],
  },
  {
    path: "/heterocloud/",
    title: "HeteroCloudの紹介",
    description: "Flash、Flash Registry、Flow、Syouyu、Secret Managerと、共通のIAM・利用量管理を紹介します。",
    lead: "HeteroCloudは、アプリケーションに必要な実行環境・通信・保存先を、共通の管理画面とAPIで扱うためのプラットフォームです。",
    sections: [
      { id: "flash", title: "FlashとFlash Registry", paragraphs: ["Flashはコンテナイメージからサービスを作り、CPU・メモリ・公開ポート・ネットワークの設定を管理する実行基盤です。Flash Registryでは、そのコンテナで使うイメージを管理できます。", "最小・最大レプリカ数やリソースの上限を指定し、用途に合わせて起動数を調整します。対応する実行環境ではGPUも選択できます。GPUの利用可否は、ハードウェアと管理者による公開・割り当て設定に依存します。"], links: [{ label: "FlashのAPIと実行時間の計測", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/blob/master/docs/FLASH_API.md" }] },
      { id: "flow", title: "Flow：アプリケーションのリアルタイム通信", paragraphs: ["ルームの管理、接続相手を見つけるためのシグナリング、STUN/TURNによる接続補助を提供します。WebRTCなどのリアルタイム通信で、クライアント間の直接接続や中継を組み合わせるために利用できます。", "接続に必要な権限はサービス単位で発行します。通信の暗号化範囲やメディアの処理方法は、アプリケーション側のプロトコルと設定も含めて確認してください。"], links: [{ label: "Flowのソースコード", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-Flow" }] },
      { id: "syouyu", title: "Syouyu：オブジェクトストレージ", paragraphs: ["バケットにファイルなどのオブジェクトを保存し、S3互換APIから読み書きできます。バケットの容量制限、アクセス用の認証情報、保存量とオブジェクト数を管理します。", "S3互換は、他社サービスのすべてのAPIや拡張機能との互換性を意味しません。使用するSDKの機能とエンドポイントを確認して接続してください。"], links: [{ label: "Syouyuのソースコード", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud-Syouyu" }] },
      { id: "secrets", title: "Secret Manager：コンテナで使う機密情報", paragraphs: ["APIキーや接続情報など、ソースコードやコンテナイメージに含めたくない値を管理します。Flashのサービス編集画面からシークレットを指定し、コンテナ内の環境変数として利用できます。", "値をログへ出力しないことや、アプリケーションに必要な権限だけを渡すことは、利用者側でも設定してください。"] },
      { id: "management", title: "組織・権限・利用量を共通管理", items: ["組織とプロジェクトで、メンバーやリソースを整理します。", "IAMポリシーとバインディングで、利用できる操作と対象を指定します。", "コスト管理ではリソース使用量を確認し、設定された上限に対する消費状況を把握します。表示される使用量や上限は、そのまま請求額を意味するものではありません。", "監査ログを使い、管理操作の履歴を確認できます。"], links: [{ label: "コンソールへ", href: "/login" }, { label: "利用を始める手順", href: "/getting-started/" }] },
    ],
  },
  {
    path: "/heteronet/",
    title: "HeteroNetの紹介",
    description: "HeteroNet（HeteroNetwork）はWireGuardを使うP2P VPNです。異なるネットワークのマシンを、直接接続・中継・レイジー接続で結びます。",
    lead: "手元のPC、サーバー、エッジ端末。置かれた場所やネットワークが違っても、必要な相手と通信できるようにします。",
    sections: [
      { id: "overlay", title: "離れたマシンをひとつのオーバーレイへ", paragraphs: ["HeteroNetはHeteroNetworkの呼び名です。Linuxノードやデスクトップクライアントを登録し、WireGuardを使って暗号化されたネットワークを構成します。物理的な接続先をアプリケーションから切り離し、仮想的なアドレスと経路で通信します。"] },
      { id: "paths", title: "直接接続を優先し、必要なときは中継", paragraphs: ["相手の到達候補を調べて直接通信できる経路を選びます。NATやファイアウォールなどの条件によって直接接続できない場合は、中継経路へ切り替えます。", "利用する通信先に応じて接続を作るレイジー接続により、全端末間に常時トンネルを張る構成を避けます。接続数や制御通信の負荷は、通信パターンや構成によって変わります。"] },
      { id: "clients", title: "PCとサーバーから利用", items: ["macOSとWindowsではデスクトップクライアントを利用できます。OSのネットワーク設定には管理者権限が必要になる場合があります。", "Linuxノードの追加は、管理コンソールが発行する登録手順に従います。登録用トークンや鍵は公開しないでください。", "DockerやKubernetesとの連携では、コンテナやサービスへ到達する経路も管理します。"], links: [{ label: "クライアントのセットアップ手順", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork#quick-setup" }, { label: "公開リリース", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/releases/latest" }] },
      { id: "cloud-relationship", title: "HeteroCloudとの関係", paragraphs: ["HeteroNetはマシン間の接続と経路を担当し、HeteroCloudは利用者・権限・サービスの管理を担当します。Flowが提供するアプリケーション向けのSTUN/TURNと、HeteroNetが提供する基盤のVPNは、役割が異なります。", "公開されたHeteroCloudの画面やサービスを使うために、常にVPN接続が必要なわけではありません。VPN専用として提供される管理画面やリソースには、登録済みのVPN接続が必要です。"], links: [{ label: "技術解説", href: "/technology/#network" }, { label: "実装状況と制約", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/blob/master/docs/IMPLEMENTATION_STATUS.md" }] },
    ],
  },
  {
    path: "/technology/",
    title: "技術解説",
    description: "HeteroCloudとHeteroNetの構成、IAM、サービスの実行、レイジー接続、使用量の計測、宣言的な運用を解説します。",
    lead: "管理する仕組みと、実際に処理・通信する仕組みを分け、それぞれの責任範囲を明確にしています。",
    sections: [
      { id: "architecture", title: "制御プレーンとデータプレーン", paragraphs: ["HeteroCloudの制御プレーンは、利用者、組織、プロジェクト、IAM、サービスの設定と状態、監査記録を管理します。Flash・Flow・Syouyuなどのプロバイダーは、実際のコンテナ実行・通信・保存を担当します。", "コンソールで設定を変更すると、APIが権限を確認し、希望する状態を保存します。ワーカーがプロバイダーへ変更を伝え、実際の状態を継続的に照合します。そのため、更新要求の受付と、実際に利用できる状態への移行には時間差があります。"], table: { headings: ["層", "主な責任"], rows: [["コンソール / CLI", "設定操作と状態・使用量の確認"], ["HeteroCloud API / IAM", "本人確認、組織境界、操作権限、設定と監査"], ["サービスプロバイダー", "コンテナ、通信セッション、オブジェクトの実処理"], ["Kubernetes / HeteroNet", "ワークロード配置、ノード接続、到達経路"]] }, links: [{ label: "アーキテクチャ文書", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/blob/master/docs/ARCHITECTURE.md" }] },
      { id: "identity", title: "ログインとアクセス制御", paragraphs: ["ブラウザーのログインはOpenID ConnectとPKCEを使ってIDプロバイダーへ接続します。ブラウザーには、管理基盤の秘密鍵やサービス内部の管理用認証情報を渡しません。", "IAMは組織の境界を確認した上でポリシーを評価します。許可がない操作は拒否し、明示的な拒否を許可より優先します。CLIのデバイスコード認証では、ブラウザーでログインして対象組織へのアクセスを承認します。", "認可判定の一部はLeanによる形式化とRustのテストで検証しています。これはシステム全体に脆弱性がないことや、すべての実行環境の安全性を証明するものではありません。"], links: [{ label: "認証・認可の設計", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/blob/master/docs/SECURITY.md" }] },
      { id: "containers", title: "Flashの実行とコールドスタート", paragraphs: ["FlashはKubernetes上でコンテナを管理します。通常のコンテナ実行ではgVisorによる分離を利用します。CPU・メモリなどの要求量や、実行環境が対応する機能に応じて配置されます。", "最小レプリカ数を0にできる構成では、停止状態から起動する際に待ち時間が発生します。これがコールドスタートです。常時起動するレプリカが必要な場合は最小数を1以上に設定しますが、障害や再配置による中断までなくなるわけではありません。", "GPU実行の可否や分離方式は、CPUのみの実行と同一とは限りません。選べるGPUの種類とアクセス範囲、利用上限はコンソールの表示を確認してください。"] },
      { id: "network", title: "WireGuard・NAT越え・レイジー接続", paragraphs: ["HeteroNetの制御プレーンは登録された端末や到達候補を管理し、データプレーンがWireGuardでパケットを運びます。直接到達できる経路を優先し、必要に応じて中継を利用します。", "レイジー接続では、通信需要を検知して必要なピアとの接続を用意します。常時接続する相手を絞る設計ですが、初回通信の経路確立には待ち時間が生じることがあります。", "端末が増えたときの性能は、同時通信する組み合わせ、NATの条件、中継比率、制御プレーンの構成にも依存します。このページでは、特定の台数や計算量を性能保証として提示しません。"], links: [{ label: "HeteroNetworkの設計", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroNetwork/blob/master/docs/ARCHITECTURE.md" }] },
      { id: "storage", title: "ストレージとシークレット", paragraphs: ["SyouyuはバケットとS3互換APIを提供し、組織・プロジェクト・サービス単位の権限で保存先を管理します。データの複製は可用性のための仕組みであり、利用者が誤って削除したデータを戻すためのバックアップとは別です。", "シークレットは専用の管理基盤に保存し、Flashの設定から必要な値を環境変数として接続します。アプリケーション自身が値をログへ出したり、外部へ送信したりしないように扱う必要があります。"] },
      { id: "usage", title: "使用量と上限", paragraphs: ["CPU・メモリなどの実行時間、通信量、保存容量は異なる指標です。CPU時間はCPU資源の使用、メモリ時間はメモリ資源と時間の積、通信量は転送したバイト数、保存容量は保存されているデータ量として区別します。集計期間・単位は各画面とAPIの定義を確認してください。", "メトリクスの取得には遅延や失敗があり得ます。取得できなかった値を実際の使用量0と同じ意味には扱わず、計測時刻と取得状態も確認します。コスト管理の値を、個別に案内された料金条件のない請求額として解釈しないでください。"] },
      { id: "operations", title: "宣言的な構成と検証", paragraphs: ["インフラやサービスの構成は、Terraform、Ansible、Helm、Argo CDなどを使って管理します。設定と実際の状態を比較し、変更内容をリポジトリで確認できる形にします。", "単体テスト、契約の検証、実環境での接続確認など、対象に応じた検証を組み合わせます。ソースコードと実装状況は公開リポジトリで確認できます。"] },
    ],
  },
  {
    path: "/getting-started/",
    title: "利用ガイド",
    description: "HeteroCloudへのログイン、組織・プロジェクトの選択、サービス作成、CLIのデバイスコード認証までの流れを紹介します。",
    lead: "コンソールで利用できるサービスと上限を確認し、小さな構成から始めます。",
    sections: [
      { id: "sign-in", title: "1. コンソールにログイン", paragraphs: ["利用するHeteroCloudのコンソールを開き、設定されているIDプロバイダーでログインします。新規登録の可否は運営者とIDプロバイダーの設定によります。招待を受けている場合は、案内された招待リンクを使ってください。"], links: [{ label: "ログイン画面へ", href: "/login" }] },
      { id: "organization", title: "2. 組織とプロジェクトを確認", paragraphs: ["操作する組織を選び、用途ごとにプロジェクトを用意します。メンバーを招待する場合は、必要なサービスと操作だけを許可してください。アカウントの作成だけで、すべてのサービスや資源が利用可能になるわけではありません。"] },
      { id: "service", title: "3. 必要なサービスを作成", items: ["コンテナを動かす場合：Flashでイメージ、CPU・メモリ、起動数、公開範囲を指定します。", "リアルタイム通信を使う場合：Flowのサービスを作り、必要な範囲の接続用認証情報を発行します。", "ファイルを保存する場合：Syouyuでバケットを作り、S3互換クライアントにエンドポイントと認証情報を設定します。", "機密情報をコンテナに渡す場合：Flashの編集画面でシークレットを環境変数に接続します。"] },
      { id: "observe", title: "4. 稼働状態と利用量を確認", paragraphs: ["サービスが利用可能な状態になったこと、想定した公開範囲になっていることを確認します。起動に失敗した場合は、状態やエラーメッセージを確認してください。使用量はコスト管理と各サービスの画面で確認できます。"] },
      { id: "cli", title: "CLIから操作する", paragraphs: ["コンソールのCLIセットアップから、OS・CPUに合ったインストール方法を確認できます。Linux・macOS・Windowsのx64とARM64向けの配布物があります。接続先は、利用するHeteroCloudのURLを指定します。", "デバイスコード認証では、CLIに表示されたURLをブラウザーで開き、コードを入力してアクセスを承認します。他人から渡されたコードを承認したり、認証情報を公開したりしないでください。"], code: "# cloud.example.com は利用する HeteroCloud のURLに置き換えてください\nexport HETEROCLOUD_ENDPOINT=https://cloud.example.com\nexport HETEROCLOUD_ORGANIZATION_ID=YOUR_ORGANIZATION_ID\nheterocloud auth login --device-code\nheterocloud auth status", links: [{ label: "CLIセットアップを開く（ログインが必要）", href: "/cli" }, { label: "CLIの配布物", href: "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/latest" }] },
      { id: "vpn", title: "VPN専用リソースに接続する場合", paragraphs: ["HeteroNetのクライアントを導入し、管理者が案内する登録と接続を行います。内部ドメインへの接続は、VPNの接続状態・経路・名前解決が揃って初めて利用できます。公開サービスの利用とは分けて確認してください。"], links: [{ label: "HeteroNetの紹介とセットアップ", href: "/heteronet/" }] },
      { id: "before-use", title: "利用前に確認すること", items: ["サービスの公開範囲とアクセス権限。", "割り当てられたCPU・メモリ・GPU・通信・容量の上限。", "重要なデータの独立したバックアップ。", "利用規約、プライバシーポリシー、適正利用ポリシー、サービス提供条件。"], links: [{ label: "規約・ポリシー", href: "/legal/" }, { label: "お問い合わせ", href: "/contact/" }] },
    ],
  },
];
