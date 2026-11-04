-- Every agent tool grant is leased: the engine renews a grant while its turn runs, and one whose
-- lease lapsed is refused, then swept away. No engine revokes grants when it starts, so a turn's
-- fresh grant is never taken from it, by its own engine or by any other. See docs/coding.md,
-- Elysium's tools.
--
-- A grant already stored gets one lease, and lapses after it unless its turn renews it.
ALTER TABLE agent_tool_grants
    ADD COLUMN expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '1 minute';
ALTER TABLE agent_tool_grants ALTER COLUMN expires_at DROP DEFAULT;
