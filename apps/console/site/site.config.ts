import type { SiteConfig } from "./types.ts";

// Public facts only. Never put credentials or private contact details here.
// Keep the legal documents as drafts until the operator has reviewed their
// substance and supplied the publication details. This does not gate login.
export const siteConfig: SiteConfig = {
  operator: {
    name: null,
    email: null,
    contactUrl: null,
    address: null,
    representative: null,
  },
  legal: {
    status: "draft",
    effectiveDate: null,
    fees: "undecided",
  },
};

export function validateSiteConfig(config: SiteConfig): void {
  if (config.operator.email && !/^[^\s<>@]+@[^\s<>@]+\.[^\s<>@]+$/.test(config.operator.email)) {
    throw new Error("The public contact email is invalid");
  }
  if (config.operator.contactUrl) {
    const url = new URL(config.operator.contactUrl);
    if (url.protocol !== "https:" || url.username || url.password) {
      throw new Error("The public contact form must use an HTTPS URL without credentials");
    }
  }
  if (config.legal.status === "published") {
    if (!config.operator.name?.trim() || !(config.operator.email || config.operator.contactUrl)
      || !config.operator.address?.trim() || config.legal.fees === "undecided"
      || !config.legal.effectiveDate || !/^\d{4}-\d{2}-\d{2}$/.test(config.legal.effectiveDate)) {
      throw new Error("Published policies require the reviewed operator, contact, address, fees and effective date");
    }
    // Paid-service disclosures need concrete prices, payment, delivery and
    // cancellation conditions. Do not publish an incomplete generic template.
    if (config.legal.fees === "paid") {
      throw new Error("Add reviewed paid-service disclosures before publishing paid-service policies");
    }
  }
}
