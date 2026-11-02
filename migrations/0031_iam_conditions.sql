-- New IAM policy syntax remains disabled until every authorization-serving
-- binary understands it. Older binaries must never ignore a condition.
ALTER TABLE cloud_security_activation
    ADD COLUMN iam_conditions_enabled boolean NOT NULL DEFAULT false;
