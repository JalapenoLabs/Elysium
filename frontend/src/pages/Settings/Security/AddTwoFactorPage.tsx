// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useSearchParams } from 'react-router'

// User interface
import { Button, Spinner } from '@heroui/react'
import { AddTwoFactorWizard } from './AddTwoFactorWizard'
import { SettingsPage } from '../SettingsPage'

// Misc
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { useRecentSignIn } from '../../../hooks/useRecentSignIn'
import { UrlTree } from '../../../urls'
import { ADD_TWO_FACTOR_PARAMS, readAddTwoFactorStep } from './addTwoFactorSteps'
import { readAuthenticator } from './securityPresentation'

// `/settings/security/two-factor/new`: turning two-factor authentication on, in three steps:
// name the app, save backup codes, then scan the app and verify it.
//
// Nothing changes until the last step. The backup codes are made in a Kratos settings flow of
// their own and wait there, unsaved, while the person downloads them; the app is verified in a
// second flow; only then are the codes saved, from their flow. Leaving at any step before that
// leaves both flows to expire in Kratos with nothing applied, which is how leaving without
// downloading the codes cancels two-factor authentication (`~/.claude/docs/ux.md`, 2FA). Both
// flows' ids are in the address with the step, so reloading picks up where the person was.
export function AddTwoFactorPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const isRecent = useRecentSignIn(UrlTree.settingsTwoFactorNew)
  const codesFlow = useKratosSettingsFlow(UrlTree.settingsTwoFactorNew, {
    flowParam: ADD_TWO_FACTOR_PARAMS.codesFlow,
  })
  const flow = codesFlow.flow
  const step = readAddTwoFactorStep(params)

  // Someone with an app changes it instead. Once the app is verified, the last step finishes
  // saving the codes, so only the earlier steps send them back.
  if (flow && step !== 'app' && readAuthenticator(flow).kind === 'enrolled') {
    return <Navigate to={UrlTree.settingsSecurity} replace />
  }

  return <SettingsPage
    title={t('security.twoFactor.addTitle')}
    parents={[{ label: t('security.title'), href: UrlTree.settingsSecurity }]}
  >
    {codesFlow.loadFailed && <div className='py-10 text-center'>
      <p className='compact text-sm text-danger'>{t('security.loadError')}</p>
      <Button size='sm' variant='outline' onPress={codesFlow.retry}>
        <span>{t('common:actions.retry')}</span>
      </Button>
    </div>}

    {(!flow || !isRecent) && !codesFlow.loadFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {flow && isRecent && <AddTwoFactorWizard codesFlow={codesFlow} step={step} />}
  </SettingsPage>
}
