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
import { Form, Label, Link, Switch } from '@heroui/react'

// Misc
import { updateWorkspaceSettings } from '../../../api/routes/userRoutes'
import { usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'
import { UrlTree } from '../../../urls'

type AccessSettings = Pick<WorkspaceSettings, 'signupOpen' | 'requireMfa'>

type Props = {
  settings: WorkspaceSettings
  // Once saved, as an edit page goes back to its list.
  onSaved: () => void
}

// Who may join, and what everyone must set up: open or close sign-up, and require an
// authenticator app of every person. Kept by the page's Save. Only the switches changed here
// are held and sent, so a change another admin makes meanwhile shows at once and is never
// undone.
export function WorkspaceAccessSettings(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const dispatch = useAppDispatch()
  const [ changes, setChanges ] = useState<Partial<AccessSettings>>({})
  const signupOpen = changes.signupOpen ?? props.settings.signupOpen
  const requireMfa = changes.requireMfa ?? props.settings.requireMfa
  // Requiring an authenticator applies to the admin turning it on too. Without one of their
  // own, they would be sent to set one up before they could reach this page again.
  const hasSecondFactor = Boolean(useAppSelector(selectMe)?.hasSecondFactor)
  const canRequireMfa = props.settings.requireMfa || hasSecondFactor

  async function save() {
    try {
      const response = await updateWorkspaceSettings(changes)
      dispatch(workspaceSettingsUpdated(response.settings))
      setChanges({})
      toast.success(t('access.saved'))
      props.onSaved()
      return true
    }
    catch (error) {
      console.debug('WorkspaceAccessSettings could not save the access settings', { error })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save,
    isDirty: signupOpen !== props.settings.signupOpen || requireMfa !== props.settings.requireMfa,
  })

  return <Form className='grid gap-6 rounded-xl border border-separator p-6 md:grid-cols-2' onSubmit={onSubmit}>
    <div>
      <Switch
        id='workspace-sign-up-open'
        isSelected={signupOpen}
        onChange={(isSelected) => setChanges({ ...changes, signupOpen: isSelected })}
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
        id='workspace-require-authenticator'
        isSelected={requireMfa}
        isDisabled={!canRequireMfa}
        onChange={(isSelected) => setChanges({ ...changes, requireMfa: isSelected })}
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
  </Form>
}
