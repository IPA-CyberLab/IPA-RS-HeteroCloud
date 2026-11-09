-- Estimates use the rate effective for the entire UTC week. Historical
-- prices are append-only once a week begins; unconfigured prices are unknown.
CREATE TABLE flash_rate_cards (
    effective_week_start bigint PRIMARY KEY CHECK ((effective_week_start - 345600) % 604800 = 0),
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    cpu_microunits_per_vcpu_hour bigint NOT NULL CHECK (cpu_microunits_per_vcpu_hour BETWEEN 0 AND 1000000000000),
    memory_microunits_per_gib_hour bigint NOT NULL CHECK (memory_microunits_per_gib_hour BETWEEN 0 AND 1000000000000),
    gpu_microunits_per_gpu_hour bigint NOT NULL CHECK (gpu_microunits_per_gpu_hour BETWEEN 0 AND 1000000000000),
    updated_by uuid REFERENCES users(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE flash_weekly_budgets (
    organization_id uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    currency text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    amount_microunits bigint NOT NULL CHECK (amount_microunits BETWEEN 1 AND 9000000000000000),
    warning_percentage smallint NOT NULL CHECK (warning_percentage BETWEEN 1 AND 100),
    updated_at timestamptz NOT NULL DEFAULT now()
);
