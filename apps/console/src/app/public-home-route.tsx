import { useEffect } from "react";
import { PUBLIC_HOME_URL } from "@/lib/public-site";

export function PublicHomeRoute() {
  useEffect(() => {
    // Browser history or an old HTML document can mount the SPA at /. Load
    // the public document before any session check, bypassing the old cache.
    // Keep a link instead of looping if a static host is misconfigured.
    if (window.location.search !== "?site=public") {
      window.location.replace(PUBLIC_HOME_URL);
    }
  }, []);

  return <a href={PUBLIC_HOME_URL}>HeteroCloudの紹介へ</a>;
}
