// Copyright © 2026 Jalapeno Labs

// Core
import { Configuration, FrontendApi } from '@ory/client-fetch'

// Misc
import { KRATOS_BASE_PATH } from '../constants'

// Ory Kratos's public API, on Elysium's own origin through nginx. Every sign-in, sign-up,
// recovery, and account change is a Kratos "flow" run through this client as JSON; Kratos
// sets the session cookie the Elysium API then reads. See docs/auth.md.
export const kratos = new FrontendApi(new Configuration({
  basePath: KRATOS_BASE_PATH,
  credentials: 'include',
  headers: {
    Accept: 'application/json',
  },
}))
