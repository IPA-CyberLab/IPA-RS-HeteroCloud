import type { ResourceQuotaTenant, ResourceQuotaUsage } from "@/lib/api-types";

export type OrganizationQuotaGroup = ResourceQuotaTenant & {
  accounts: ResourceQuotaTenant[];
};

/** Combine account allocations while preserving unknown measured usage. */
export function groupOrganizationQuotas(tenants: ResourceQuotaTenant[]): OrganizationQuotaGroup[] {
  const groups = new Map<string, ResourceQuotaTenant[]>();
  for (const tenant of tenants) {
    const key = tenant.cloud_organization_id ?? tenant.organization.id;
    const accounts = groups.get(key) ?? [];
    accounts.push(tenant);
    groups.set(key, accounts);
  }
  return [...groups.values()].map((accounts) => {
    const management = accounts.find((account) => account.organization.id === account.management_account_id) ?? accounts[0];
    const usage: ResourceQuotaUsage = { ...accounts[0].usage };
    const output = usage as unknown as Record<string, number | null>;
    for (const account of accounts.slice(1)) {
      for (const key of Object.keys(usage) as (keyof ResourceQuotaUsage)[]) {
        const current = usage[key];
        const next = account.usage[key];
        output[key] = current === null || next === null ? null
          : key.includes("_max_") ? Math.max(current, next) : current + next;
      }
    }
    return {
      ...management,
      organization: { ...management.organization, name: management.cloud_organization_name ?? management.organization.name },
      usage,
      accounts,
    };
  }).sort((left, right) => left.organization.name.localeCompare(right.organization.name));
}
