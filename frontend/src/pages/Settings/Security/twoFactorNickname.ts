// Copyright © 2026 Jalapeno Labs

// Core
import { mutate } from 'swr'

// Misc
import { clearAuthenticatorNickname, setAuthenticatorNickname } from '../../../api/routes/authRoutes'

// Saves what this person calls their authenticator app, or forgets it when blank, so a new
// app never shows an old app's name. `/me` carries it, so it is read again.
export async function storeAuthenticatorNickname(nickname: string) {
  if (nickname) {
    await setAuthenticatorNickname(nickname)
  }
  else {
    await clearAuthenticatorNickname()
  }
  await mutate('v1/me')
}
