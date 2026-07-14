-- Widen the login-history method vocabulary for password and TOTP sign-ins.
--
-- 0004 constrained `method` to just 'passkey'; OPAQUE password auth and the TOTP
-- second factor now record their own attempts (docs/auth-model.md).

ALTER TABLE login_events DROP CONSTRAINT login_events_method_check;

ALTER TABLE login_events
    ADD CONSTRAINT login_events_method_check
    CHECK (method IN ('passkey', 'password', 'totp'));
