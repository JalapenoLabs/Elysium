# Organization administration

The enterprise admin surface: the controls an org owner/admin uses to run their
organization, modeled on what Google Workspace, Microsoft 365, and Slack give
admins. The goal is to replace those suites, so this grows into a full admin
console. Authorization for every control is the org RBAC guard (`authz.md`).

## The six control planes

Every mature admin console is really six planes; this is Elysium's map of them.

1. **Authentication & security policy** — sign-in methods, mandatory 2FA, session
   lifetime, SSO, session revocation.
2. **Member lifecycle & roles** — invite, roles, suspend/reactivate, guests,
   delegated/custom admin roles, ownership transfer, offboarding.
3. **Organizational structure** — teams today; org-unit hierarchy with policy
   inheritance and dynamic groups later.
4. **Domains & identity** — verified domains, org-issued addresses, domain
   auto-join (rides the planned SMTP layer).
5. **Governance, audit & compliance** — org-wide audit log, retention, export,
   app/integration governance.
6. **Billing & branding** — branding is built; billing is deferred (self-hosted).

## Built today

- **Security policy control plane** (`policies.rs`): `GET`/`PATCH
  /api/v1/orgs/:id/auth-policy` (admin) over `allow_passkeys` / `allow_passwords`
  / `allow_magic_links`, `require_totp`, and `session_lifetime_seconds`, validated
  so an org can never disable every method or set an out-of-range lifetime.
- **Enforcement**: a disabled password method is refused at sign-in; the strictest
  `session_lifetime_seconds` across a member's orgs bounds their session at
  creation; `require_totp` surfaces as `totp_enrollment_required` in `/context`,
  and the client hard-gates such a member into TOTP enrollment
  (`POST /api/v1/me/totp/{begin,confirm}`).
- **Session revocation**: `POST /api/v1/orgs/:id/members/:id/revoke-sessions`
  (admin, must outrank the target) signs a member out everywhere and force-drops
  their live SSE.
- **Member management**: roster, role changes, removal, with the no-escalation
  rules (see `authz.md`).
- **Teams**: create / rename / delete + rosters.
- **Branding / white-label**: accent color, wordmark, logo.

## Roadmap

- **Member lifecycle**: suspend / reactivate (the `/suspended` screen exists),
  ownership transfer, a guest role, and self-service "leave an org".
- **Delegated / custom admin roles**: security-admin, user-admin, billing-admin,
  the granular privilege model beyond owner/admin/member.
- **SSO / SAML + OIDC and SCIM provisioning**: bring-your-own IdP and automatic
  user provisioning/deprovisioning. The largest enterprise unlock; wants its own
  design pass.
- **Org units + policy inheritance**, dynamic groups, IP allowlisting.
- **Verified domains + org-issued addresses + domain auto-join** (with the SMTP
  layer).
- **Org-wide audit log**, retention, and export.
- **Backend teeth for `require_totp`**: today the enrollment gate is
  client-enforced from the `/context` signal; a server-side block of org access
  for an un-enrolled member is the residual hardening.
