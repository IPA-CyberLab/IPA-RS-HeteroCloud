import { QueryClientProvider } from "@tanstack/react-query";
import { lazy, Suspense } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "@/components/layout/app-shell";
import { PageLoading } from "@/components/shared/page-loading";
import { ProtectedRoute } from "@/features/auth/protected-route";
import { LoginPage } from "@/features/auth/login-page";
import { RegisterPage } from "@/features/auth/register-page";
import { OrganizationProvider } from "@/features/organizations/organization-context";
import { createQueryClient } from "@/lib/query-client";
import { NotFoundPage } from "@/app/not-found-page";

const queryClient = createQueryClient();
const OverviewPage = lazy(() =>
  import("@/features/overview/overview-page").then((module) => ({
    default: module.OverviewPage,
  })),
);
const OrganizationsPage = lazy(() =>
  import("@/features/organizations/organizations-page").then((module) => ({
    default: module.OrganizationsPage,
  })),
);
const ProjectsPage = lazy(() =>
  import("@/features/projects/projects-page").then((module) => ({
    default: module.ProjectsPage,
  })),
);
const IamPrincipalsPage = lazy(() =>
  import("@/features/iam/principals-page").then((module) => ({
    default: module.IamPrincipalsPage,
  })),
);
const IamBindingsPage = lazy(() =>
  import("@/features/iam/bindings-page").then((module) => ({
    default: module.IamBindingsPage,
  })),
);
const IamPoliciesPage = lazy(() =>
  import("@/features/iam/policies-page").then((module) => ({
    default: module.IamPoliciesPage,
  })),
);
const RealtimeServicesPage = lazy(() =>
  import("@/features/realtime/realtime-services-page").then((module) => ({
    default: module.RealtimeServicesPage,
  })),
);
const RealtimeServiceDetailPage = lazy(() =>
  import("@/features/realtime/realtime-service-detail-page").then((module) => ({
    default: module.RealtimeServiceDetailPage,
  })),
);
const AuditLogsPage = lazy(() =>
  import("@/features/audit/audit-logs-page").then((module) => ({
    default: module.AuditLogsPage,
  })),
);
const SettingsPage = lazy(() =>
  import("@/features/settings/settings-page").then((module) => ({
    default: module.SettingsPage,
  })),
);

function LazyPage({ children }: { children: React.ReactNode }) {
  return (
    <Suspense fallback={<PageLoading label="画面を読み込んでいます" />}>
      {children}
    </Suspense>
  );
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/register" element={<RegisterPage />} />
          <Route element={<ProtectedRoute />}>
            <Route element={<OrganizationProvider />}>
              <Route element={<AppShell />}>
              <Route index element={<Navigate to="/overview" replace />} />
              <Route
                path="/overview"
                element={
                  <LazyPage>
                    <OverviewPage />
                  </LazyPage>
                }
              />
              <Route
                path="/organizations"
                element={
                  <LazyPage>
                    <OrganizationsPage />
                  </LazyPage>
                }
              />
              <Route
                path="/projects"
                element={
                  <LazyPage>
                    <ProjectsPage />
                  </LazyPage>
                }
              />
              <Route
                path="/iam"
                element={<Navigate to="/iam/principals" replace />}
              />
              <Route
                path="/iam/users"
                element={<Navigate to="/iam/principals" replace />}
              />
              <Route
                path="/iam/principals"
                element={
                  <LazyPage>
                    <IamPrincipalsPage />
                  </LazyPage>
                }
              />
              <Route
                path="/iam/roles"
                element={<Navigate to="/iam/bindings" replace />}
              />
              <Route
                path="/iam/bindings"
                element={
                  <LazyPage>
                    <IamBindingsPage />
                  </LazyPage>
                }
              />
              <Route
                path="/iam/policies"
                element={
                  <LazyPage>
                    <IamPoliciesPage />
                  </LazyPage>
                }
              />
              <Route
                path="/realtime"
                element={<Navigate to="/realtime/services" replace />}
              />
              <Route
                path="/realtime/services"
                element={
                  <LazyPage>
                    <RealtimeServicesPage />
                  </LazyPage>
                }
              />
              <Route
                path="/realtime/services/:serviceId"
                element={
                  <LazyPage>
                    <RealtimeServiceDetailPage />
                  </LazyPage>
                }
              />
              <Route
                path="/audit-logs"
                element={
                  <LazyPage>
                    <AuditLogsPage />
                  </LazyPage>
                }
              />
              <Route
                path="/settings"
                element={
                  <LazyPage>
                    <SettingsPage />
                  </LazyPage>
                }
              />
              </Route>
            </Route>
          </Route>
          <Route path="*" element={<NotFoundPage />} />
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  );
}
