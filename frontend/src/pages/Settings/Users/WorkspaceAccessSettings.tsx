// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Label, Link, Switch } from '@heroui/react'

// Misc
import { UrlTree } from '../../../urls'

export type AccessSettings = Pick<WorkspaceSettings, 'signupOpen' | 'requireMfa'>

type Props = {
  settings: WorkspaceSettings
  // The switches changed on this page and not yet saved.
  changes: Partial<AccessSettings>
  onChange: (changes: Partial<AccessSettings>) => void
}

// Who may join, and what everyone must set up: open or close sign-up, and require an
// authenticator app of every person. Fields of the Users page, kept by its Save.
export function WorkspaceAccessSettings(props: Props) {
  const { t } = useTranslation('users')
  const signupOpen = props.changes.signupOpen ?? props.settings.signupOpen
  const requireMfa = props.changes.requireMfa ?? props.settings.requireMfa
  // Requiring an authenticator applies to the admin turning it on too. Without one of their
  // own, they would be sent to set one up before they could reach this page again.
  const hasSecondFactor = Boolean(useAppSelector(selectMe)?.hasSecondFactor)
  const canRequireMfa = props.settings.requireMfa || hasSecondFactor

  return <div className='relaxed grid gap-6 rounded-xl border border-separator p-6 md:grid-cols-2'>
    <div>
      <Switch
        isSelected={signupOpen}
        onChange={(isSelected) => props.onChange({ ...props.changes, signupOpen: isSelected })}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('access.signupOpen')}</Label>
        </Switch.Content>
      </Switch>
      <p className='mt-1 text-sm opacity-70'>{t('access.signupOpenHint')}</p>
    </div>
    <div>
      <Switch
        isSelected={requireMfa}
        isDisabled={!canRequireMfa}
        onChange={(isSelected) => props.onChange({ ...props.changes, requireMfa: isSelected })}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('access.requireMfa')}</Label>
        </Switch.Content>
      </Switch>
      <p className='mt-1 text-sm opacity-70'>{t('access.requireMfaHint')}</p>
      {!canRequireMfa && <p className='mt-1 text-sm text-warning'>
        <span>{t('access.requireMfaFirst')}</span>
        {' '}
        <Link href={UrlTree.settingsSecurity} className='text-link'>{t('access.setUpYours')}</Link>
      </p>}
    </div>
  </div>
}
