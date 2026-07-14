// Copyright © 2026 Jalapeno Labs

// Core
import { useEnrollCredential } from './useEnrollCredential'

// Misc
import { bootstrapBegin, bootstrapFinish } from '../../api/routes/authRoutes'

// The identity the first-run owner enters in the wizard. `givenName`/`familyName`
// are the full name; `displayName` is what the UI shows and the server defaults to
// `givenName` when it is blank (see docs/identity.md).
export type ClaimDetails = {
  orgName: string
  email: string
  givenName: string
  familyName: string
  displayName: string
}

// First-run claim, as a thin wiring of the shared enrollment machine
// ([`useEnrollCredential`]) to the bootstrap endpoints. Exposes the same surface
// (`enrollPasskey` / `beginPassword` / `completePassword` / `provisioningUri` /
// `recoveryCodes` / `isBusy` / `error`); the invite-accept and add-credential
// hooks are the same wrapper against their own endpoints.
export function useClaimInstance(details: ClaimDetails) {
  return useEnrollCredential({
    begin: (method, opaqueRequest) => bootstrapBegin({
      method,
      org_name: details.orgName.trim(),
      email: details.email.trim(),
      given_name: details.givenName.trim(),
      family_name: details.familyName.trim(),
      display_name: details.displayName.trim(),
      opaque_registration_request: opaqueRequest,
    }),
    finish: (body) => bootstrapFinish(body),
  })
}
