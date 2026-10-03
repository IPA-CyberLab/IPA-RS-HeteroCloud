export interface SiteLink {
  label: string;
  href: string;
}

export interface SiteSection {
  id: string;
  title: string;
  paragraphs?: string[];
  items?: string[];
  links?: SiteLink[];
  table?: { headings: string[]; rows: string[][] };
  code?: string;
}

export interface SitePage {
  path: string;
  title: string;
  description: string;
  lead: string;
  sections: SiteSection[];
  legal?: boolean;
}

export interface SiteConfig {
  operator: {
    name: string | null;
    email: string | null;
    contactUrl: string | null;
    address: string | null;
    representative: string | null;
  };
  legal: {
    status: "draft" | "published";
    effectiveDate: string | null;
    fees: "undecided" | "free" | "paid";
  };
}
