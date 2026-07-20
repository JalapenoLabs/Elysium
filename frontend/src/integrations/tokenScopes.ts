// Copyright © 2026 Jalapeno Labs

// Misc
import { TokenScope } from '../proto/generated/security_pb'

// The scopes a minted access token can grant (#34, docs/integrations.md).
// Labels live in common.jsonc under `token.scopes.*`; the enum is the source of
// truth for which scopes exist and are offered in the mint form.
export const selectableTokenScopes = [
  TokenScope.GIT_READ,
  TokenScope.GIT_WRITE,
  TokenScope.REGISTRY_PULL,
  TokenScope.REGISTRY_PUSH,
  TokenScope.MAIL_SEND,
] as const

export const tokenScopeLabelKey = {
  [TokenScope.UNSPECIFIED]: 'token.scopes.unspecified',
  [TokenScope.GIT_READ]: 'token.scopes.gitRead',
  [TokenScope.GIT_WRITE]: 'token.scopes.gitWrite',
  [TokenScope.REGISTRY_PULL]: 'token.scopes.registryPull',
  [TokenScope.REGISTRY_PUSH]: 'token.scopes.registryPush',
  [TokenScope.MAIL_SEND]: 'token.scopes.mailSend',
} as const satisfies Record<TokenScope, string>
