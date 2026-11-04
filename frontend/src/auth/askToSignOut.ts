// Copyright © 2026 Jalapeno Labs

import type { ConfirmOptions } from '../gates/ConfirmGate'

// Core
import i18next from 'i18next'

// Misc
import { signOut } from './signOut'

// Sign out asks first, wherever it is chosen: the account menu or search.
export function askToSignOut(confirm: (options: ConfirmOptions) => void) {
  confirm({
    title: i18next.t('topbar.signOutConfirm.title', { ns: 'navigation' }),
    message: i18next.t('topbar.signOutConfirm.message', { ns: 'navigation' }),
    confirmText: i18next.t('topbar.signOut', { ns: 'navigation' }),
    // The page reloads onto the sign-in page, so the dialog never needs closing.
    onConfirm: signOut,
  })
}
