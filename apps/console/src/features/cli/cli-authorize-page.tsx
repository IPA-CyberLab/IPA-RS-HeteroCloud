import Alert from "@cloudscape-design/components/alert";
import Box from "@cloudscape-design/components/box";
import Button from "@cloudscape-design/components/button";
import Container from "@cloudscape-design/components/container";
import FormField from "@cloudscape-design/components/form-field";
import Header from "@cloudscape-design/components/header";
import Input from "@cloudscape-design/components/input";
import KeyValuePairs from "@cloudscape-design/components/key-value-pairs";
import SpaceBetween from "@cloudscape-design/components/space-between";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useSearchParams } from "react-router-dom";
import { ErrorState } from "@/components/shared/error-state";
import { PageHeader } from "@/components/shared/page-header";
import { PageLoading } from "@/components/shared/page-loading";
import { api, getApiErrorMessage } from "@/lib/api-client";

export function CliAuthorizePage() {
  const [searchParams] = useSearchParams();
  const [userCode, setUserCode] = useState(searchParams.get("user_code")?.trim() ?? "");
  const [enteredCode, setEnteredCode] = useState("");
  const [codeError, setCodeError] = useState("");
  const submitCode = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const characters = enteredCode.trim().replaceAll("-", "").toUpperCase();
    if (!/^[A-Z0-9]{12}$/.test(characters)) {
      setCodeError("CLIに表示された12文字の確認コードを入力してください。");
      return;
    }
    setCodeError("");
    setUserCode(`${characters.slice(0, 4)}-${characters.slice(4, 8)}-${characters.slice(8)}`);
  };
  const authorization = useQuery({
    queryKey: ["auth", "cli-device", userCode],
    queryFn: ({ signal }) => api.auth.cliDevice.get(userCode, signal),
    enabled: userCode.length > 0,
    retry: false,
  });
  const approve = useMutation({
    mutationFn: () => api.auth.cliDevice.approve(userCode),
  });

  if (!userCode) {
    return (
      <SpaceBetween size="l">
        <PageHeader
          title="CLIの確認コードを入力"
          description="CLIに表示されたURLを開き、確認コードを入力してください。"
        />
        <Container>
          <form onSubmit={submitCode}>
            <SpaceBetween size="m">
              <FormField label="確認コード" errorText={codeError || undefined}>
                <Input
                  value={enteredCode}
                  onChange={({ detail }) => setEnteredCode(detail.value)}
                  placeholder="ABCD-EFGH-JKLM"
                  autoComplete={false}
                />
              </FormField>
              <Button variant="primary" formAction="submit">
                続行
              </Button>
            </SpaceBetween>
          </form>
        </Container>
      </SpaceBetween>
    );
  }
  if (authorization.isPending) {
    return <PageLoading label="CLI認証リクエストを確認しています" />;
  }
  if (authorization.isError) {
    return (
      <SpaceBetween size="l">
        <ErrorState
          title="CLI認証リクエストを確認できません"
          description={getApiErrorMessage(authorization.error)}
          onRetry={() => void authorization.refetch()}
        />
        <Button onClick={() => setUserCode("")}>別の確認コードを入力</Button>
      </SpaceBetween>
    );
  }

  return (
    <SpaceBetween size="l">
      <PageHeader
        title="CLIを承認"
        description="この端末のHeteroCloud CLIへ、選択した組織の権限を渡します。"
      />
      {approve.isSuccess ? (
        <Alert type="success" header="CLIを承認しました">
          CLIへ戻ってください。認証情報の保存と接続確認が自動で完了します。この画面は閉じて構いません。
        </Alert>
      ) : (
        <Container
          header={
            <Header
              variant="h2"
              description="CLIに表示されたコードと一致することを確認してください。"
            >
              端末からのログイン要求
            </Header>
          }
        >
          <SpaceBetween size="l">
            <KeyValuePairs
              columns={1}
              items={[
                {
                  label: "確認コード",
                  value: <Box variant="code">{authorization.data.user_code}</Box>,
                },
                {
                  label: "組織",
                  value: `${authorization.data.organization.organization_name} (${authorization.data.organization.organization_slug})`,
                },
                {
                  label: "有効期限",
                  value: new Date(authorization.data.expires_at).toLocaleString(),
                },
              ]}
            />
            <Alert type="warning" header="自分で開始した操作だけ承認してください">
              承認後、CLIはこの組織であなたと同じIAM権限を使用できます。アクセストークンはブラウザには表示されません。
            </Alert>
            {approve.isError ? (
              <Alert type="error" header="承認できませんでした">
                {getApiErrorMessage(approve.error)}
              </Alert>
            ) : null}
            <SpaceBetween direction="horizontal" size="xs">
              <Button
                variant="primary"
                iconName="check"
                loading={approve.isPending}
                onClick={() => approve.mutate()}
              >
                このCLIを承認
              </Button>
              <Button href="/cli">キャンセル</Button>
            </SpaceBetween>
          </SpaceBetween>
        </Container>
      )}
    </SpaceBetween>
  );
}
