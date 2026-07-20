// Copyright © 2026 Jalapeno Labs

import type { PaletteEntry } from './registry'

// Core
import { useMemo } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Misc
import { usePaletteEntries } from './registry'
import { UrlTree } from '../../urls'

// The app's core navigation targets, contributed to the command palette from
// the app shell. Pillars register their own entries the same way (see
// registry.ts); this hook only owns the destinations every workspace has.
export function useNavigationPaletteEntries() {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const entries = useMemo<PaletteEntry[]>(() => [
    {
      id: 'nav.home',
      label: t('palette.nav.home'),
      section: t('palette.sections.navigate'),
      run: () => navigate(UrlTree.home),
    },
    {
      id: 'nav.settings',
      label: t('palette.nav.settings'),
      keywords: t('palette.nav.settingsKeywords'),
      section: t('palette.sections.navigate'),
      run: () => navigate(UrlTree.settings),
    },
    {
      id: 'nav.organization',
      label: t('palette.nav.organization'),
      keywords: t('palette.nav.organizationKeywords'),
      section: t('palette.sections.navigate'),
      run: () => navigate(UrlTree.organization),
    },
    {
      id: 'nav.organizationMembers',
      label: t('palette.nav.organizationMembers'),
      section: t('palette.sections.navigate'),
      run: () => navigate(UrlTree.organizationMembers),
    },
    {
      id: 'nav.organizationTeams',
      label: t('palette.nav.organizationTeams'),
      section: t('palette.sections.navigate'),
      run: () => navigate(UrlTree.organizationTeams),
    },
  ], [ t, navigate ])

  usePaletteEntries('app.navigation', entries)
}
