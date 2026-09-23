const RETURN_PATH_KEY = "heterocloud.auth-return-path";

function safeInternalPath(value: string | null): string | null {
  if (!value || !value.startsWith("/") || value.startsWith("//")) return null;
  try {
    const parsed = new URL(value, window.location.origin);
    if (parsed.origin !== window.location.origin) return null;
    return `${parsed.pathname}${parsed.search}${parsed.hash}`;
  } catch {
    return null;
  }
}

export function rememberAuthReturnPath(path: string): void {
  const safePath = safeInternalPath(path);
  if (!safePath || safePath === "/login") return;
  try {
    window.sessionStorage.setItem(RETURN_PATH_KEY, safePath);
  } catch {
    // Browser storage can be disabled; login still works with the default route.
  }
}

export function readAuthReturnPath(): string | null {
  try {
    return safeInternalPath(window.sessionStorage.getItem(RETURN_PATH_KEY));
  } catch {
    return null;
  }
}

export function clearAuthReturnPath(): void {
  try {
    window.sessionStorage.removeItem(RETURN_PATH_KEY);
  } catch {
    // Browser storage can be disabled.
  }
}
