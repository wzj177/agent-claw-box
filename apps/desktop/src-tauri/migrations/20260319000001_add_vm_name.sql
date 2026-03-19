ALTER TABLE agents ADD COLUMN vm_name TEXT NOT NULL DEFAULT '';

UPDATE agents
SET vm_name = container_name
WHERE vm_name = '';

CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_vm_name ON agents(vm_name);