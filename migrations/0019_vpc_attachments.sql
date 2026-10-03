-- Tenant-serialized reference checks enforce VPC ownership and lifecycle.
CREATE INDEX service_instances_flash_vpc_idx
    ON service_instances (organization_id, (spec #>> '{network,vpc_id}'))
    WHERE provider = 'flash' AND spec ? 'network';
CREATE UNIQUE INDEX service_instances_flash_vpc_name_idx
    ON service_instances (organization_id, (spec #>> '{network,vpc_id}'),
       (COALESCE(spec #>> '{network,private_name}', 'f-' || replace(id::text, '-', ''))))
    WHERE provider = 'flash' AND spec #>> '{network,vpc_id}' IS NOT NULL;
