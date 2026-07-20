-- Orchestration: persist each managed integration's published host port.
--
-- The reconciler publishes a managed HTTP surface (Forgejo's web UI today) on a
-- host port drawn from a fixed band. A persisted per-integration allocation
-- replaces the former hash-of-id derivation, whose birthday-problem collisions
-- could fold two integrations on one host onto the same port and fail the second
-- container's publish (port already in use). The port is claimed from the band on
-- first provision, stays fixed across restarts (no container churn), and is freed
-- automatically when the integration row is deleted, because it travels with the
-- row (integrations cascade-delete; there is no soft delete).
--
-- One column, not a side table: an integration publishes at most one HTTP surface
-- today. Should a kind ever need several published ports, this becomes a child
-- table (integration_ports) keyed by (integration_id, role).

ALTER TABLE integrations
    -- The host port this integration's HTTP surface is published on, within the
    -- managed band [20000, 31999]. Kept in sync with MANAGED_HOST_PORT_BASE and
    -- MANAGED_HOST_PORT_SPAN in backend/src/orchestration/reconcile.rs (base 20000,
    -- span 12000, so the inclusive upper bound is 31999). NULL until first
    -- allocated, and permanently for kinds that publish no surface.
    ADD COLUMN host_port INTEGER
        CHECK (host_port IS NULL OR host_port BETWEEN 20000 AND 31999);

-- At most one integration holds any given host port on this deployment host. The
-- partial index skips the many NULLs (unallocated, or kinds with no surface) and
-- is what makes the lowest-free allocation collision-free: a concurrent
-- double-allocation of the same port loses to this index and retries onto the next
-- free port.
CREATE UNIQUE INDEX integrations_host_port_key
    ON integrations (host_port)
    WHERE host_port IS NOT NULL;
