// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'
import { workspaceSettingsUpdated } from '../../../store/workspaceSettingsSlice'

// User interface
import { Label, Link, Switch, toast } from '@heroui/react'

// Misc
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { UrlTree } from '../../../urls'

type Props = {
  settings: WorkspaceSettings
}

type SettingKey = 'signupOpen' | 'requireMfa'

// Who may join, and what everyone must set up: open or close sign-up, and require an
// authenticator app of every person.
export function WorkspaceAccessSettings(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const dispatch = useAppDispatch()
  const [ saving, setSaving ] = useState<SettingKey | null>(null)
  // Requiring an authenticator applies to the admin turning it on too. Without one of their
  // own, they would be sent to set one up before they could reach this page again.
  const hasSecondFactor = Boolean(useAppSelector(selectMe)?.hasSecondFactor)
  const canRequireMfa = props.settings.requireMfa || hasSecondFactor

  async function save(key: SettingKey, value: boolean) {
    setSaving(key)
    try {
      const response = await updateWorkspaceSettings(key === 'signupOpen'
        ? { signupOpen: value }
        : { requireMfa: value })
      dispatch(workspaceSettingsUpdated(response.settings))
      toast.success(t('access.saved'))
    }
    catch (error) {
      console.debug('WorkspaceAccessSettings could not save a setting', { error, key })
      toast.danger(t('common:errors.unexpected'))
    }
    setSaving(null)
  }

  return <div className='relaxed grid gap-6 rounded-xl border border-separator p-6 md:grid-cols-2'>
    <div>
      <Switch
        isSelected={props.settings.signupOpen}
        isDisabled={saving === 'signupOpen'}
        onChange={(isSelected) => save('signupOpen', isSelected)}
      >
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Switch.Content>
          <Label>{t('access.signupOpen')}</Label>
        </Switch.Content>
      </Switch>
      <p className='mt-1 text-sm opacity-70'>{t('access.signupOpenHint')}</p>
    </div>
    <div>
      <Switch
        isSelected={props.settings.requireMfa}
        isDisabled={saving === 'requireMfa' || !canRequireMfa}
        onChange={(isSelected) => save('requireMfa', isSelected)}
      >
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
        <Switch.Content>
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
