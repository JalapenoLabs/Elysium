// Copyright © 2026 Jalapeno Labs

import type { UpdatesStatus } from '../../../api/routes/updateRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { updatesStatusUpdated } from '../../../store/updatesSlice'

// User interface
import { Alert, Button, Chip, toast } from '@heroui/react'
import { LuRefreshCw } from 'react-icons/lu'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { applyNewestRelease, checkForUpdates } from '../../../api/routes/updateRoutes'
import {
  isUpdaterMissing,
  releaseInQuestion,
  stateChipColors,
  stateDescriptionKeys,
  stateLabelKeys,
} from './updatesPresentation'

type Props = {
  status: UpdatesStatus
}

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

// The release running, where auto-update stands, and the two things an admin can do about
// it: check Docker Hub now, or ask for a release that rolled back here before.
export function UpdatesOverview(props: Props) {
  const { t } = useTranslation([ 'updates', 'common' ])
  const dispatch = useAppDispatch()
  const [ busyAction, setBusyAction ] = useState<'check' | 'apply' | null>(null)
  const status = props.status

  async function run(action: 'check' | 'apply') {
    setBusyAction(action)
    try {
      const response = action === 'check'
        ? await checkForUpdates()
        : await applyNewestRelease()
      dispatch(updatesStatusUpdated(response.updates))
      if (action === 'apply') {
        toast.success(t('toasts.applyRequested'))
      }
    }
    catch (error) {
      console.debug('UpdatesOverview could not complete an action', { error, action })
      toast.danger(t('toasts.failed'), {
        description: getUpstreamErrorMessage(error) ?? t('common:errors.unexpected'),
      })
    }
    setBusyAction(null)
  }

  let checkText = t('check.never')
  if (status.lastCheckError) {
    checkText = t('check.failed', { error: status.lastCheckError })
  }
  else if (status.lastCheckedAt) {
    checkText = t('check.lastChecked', {
      checkedAt: dateTimeFormatter.format(new Date(status.lastCheckedAt)),
    })
  }

  return <section className='relaxed rounded-xl border border-separator p-6'>
    <div className='compact flex flex-wrap items-start justify-between gap-4'>
      <div>
        <p className='text-sm opacity-70'>{t('running.label')}</p>
        <p className='text-2xl font-semibold'>{
          status.release ?? t('running.development')
        }</p>
        <p className='text-sm opacity-70'>{
          t('running.build', {
            commit: status.commit ?? t('running.unknownCommit'),
            builtAt: dateTimeFormatter.format(new Date(status.builtAt)),
          })
        }</p>
      </div>
      <Chip size='sm' variant='soft' color={stateChipColors[status.state]}>{
        t(stateLabelKeys[status.state])
      }</Chip>
    </div>

    <p className='compact'>{
      t(stateDescriptionKeys[status.state], {
        release: releaseInQuestion(status),
        count: status.waitingFor.length,
      })
    }</p>

    {isUpdaterMissing(status) && <Alert className='compact' status='warning'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{t('updater.missingTitle')}</Alert.Title>
        <Alert.Description>{t('updater.missingDescription')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    <div className='flex flex-wrap items-center justify-between gap-3'>
      <p className='text-sm opacity-70'>{checkText}</p>
      <div className='flex gap-2'>
        {status.state === 'held-back' && <Button
          isPending={busyAction === 'apply'}
          isDisabled={busyAction !== null}
          onPress={() => run('apply')}
        >
          <span>{t('actions.updateNow', { release: releaseInQuestion(status) })}</span>
        </Button>}
        <Button
          variant='outline'
          isPending={busyAction === 'check'}
          isDisabled={busyAction !== null || status.state === 'not-a-release'}
          onPress={() => run('check')}
        >
          <LuRefreshCw />
          <span>{t('actions.checkNow')}</span>
        </Button>
      </div>
    </div>
  </section>
}
