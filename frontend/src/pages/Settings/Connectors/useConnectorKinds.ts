// Copyright © 2026 Jalapeno Labs

import type { AccountKind, ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import useSWR from 'swr'

// Misc
import { listConnectorKinds } from '../../../api/routes/connectorRoutes'

// Every connector kind and account kind, as the API defines them. They are fixed for a build,
// so SWR's buffered copy is enough and nothing keeps them in Redux.
export function useConnectorKinds() {
  const { data, error } = useSWR('v1/connectors/kinds', listConnectorKinds)

  function kindOf(kind: ConnectorKind) {
    return data?.kinds.find((definition) => definition.kind === kind) ?? null
  }

  function accountOf(kind: AccountKind) {
    return data?.accounts.find((definition) => definition.kind === kind) ?? null
  }

  return {
    kinds: data?.kinds ?? [],
    isLoading: !data && !error,
    error,
    kindOf,
    accountOf,
  } as const
}
