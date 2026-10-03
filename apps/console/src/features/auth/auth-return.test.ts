import { beforeEach, describe, expect, it } from "vitest";
import { clearAuthReturnPath, readAuthReturnPath, rememberAuthReturnPath } from "./auth-return";

describe("post-login return destination", () => {
  beforeEach(clearAuthReturnPath);

  it("preserves a device authorization request through the console entry", () => {
    rememberAuthReturnPath("/cli/authorize?user_code=ABCD-EFGH");
    rememberAuthReturnPath("/console");
    expect(readAuthReturnPath()).toBe("/cli/authorize?user_code=ABCD-EFGH");
    clearAuthReturnPath();
    expect(readAuthReturnPath()).toBeNull();
  });

  it.each(["/", "/console", "/console/", "/console?from=login", "/login", "//outside.example/"])(
    "does not loop through or leave the entry page: %s", (value) => {
      rememberAuthReturnPath(value);
      expect(readAuthReturnPath()).toBeNull();
      // Existing browser state from older releases is filtered as well.
      sessionStorage.setItem("heterocloud.auth-return-path", value);
      expect(readAuthReturnPath()).toBeNull();
    },
  );
});
