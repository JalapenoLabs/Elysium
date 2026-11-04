// Copyright © 2026 Jalapeno Labs

import type { ProjectManager } from '../../api/routes/projectManagerRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectManagerLoaded } from '../../store/projectManagersSlice'

// User interface
import { Button, Label, Switch, useOverlayState } from '@heroui/react'
import { HelpPopover } from '../../components/HelpPopover'
import { TrackerModal } from './TrackerModal'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { setProjectManagerWritesDirectly } from '../../api/routes/projectManagerRoutes'
import { providerLabelKeys } from '../ActionItems/linkPresentation'
import { trackerSourceLabelKeys } from './trackerPresentation'
import { toast } from '../../toast/toast'

type Props = {
  manager: ProjectManager
}

// How the project's Project Manager writes: whether its changes apply as it makes them or wait
// for a person's approval, and the tracker its issues file in, with a way to change either.
export function ProjectManagerWrites(props: Props) {
  const { t } = useTranslation([ 'projects', 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const trackerState = useOverlayState()
  const [ isSaving, setIsSaving ] = useState(false)
  const writes = props.manager.writes
  const tracker = writes.tracker

  async function setDirect(writesDirectly: boolean) {
    setIsSaving(true)
    try {
      const response = await setProjectManagerWritesDirectly(props.manager.projectId, writesDirectly)
      dispatch(projectManagerLoaded(response.projectManager))
      toast.success(t('manager.writes.saved'))
    }
    catch (error) {
      console.debug('ProjectManagerWrites failed to change how it writes', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  return <div className='relaxed flex flex-col gap-2 rounded-xl border border-separator p-4 text-sm'>
    <div className='flex items-center gap-1'>
      <Switch
        id='project-manager-writes-directly'
        isSelected={writes.direct}
        isDisabled={isSaving}
        onChange={(isSelected) => void setDirect(isSelected)}
      >
        <Switch.Content>
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
          <Label>{t('manager.writes.direct')}</Label>
        </Switch.Content>
      </Switch>
      <HelpPopover label={t('manager.writes.directHelpLabel')}>
        <p>{t('manager.writes.directHelp')}</p>
      </HelpPopover>
    </div>

    <div className='level gap-3'>
      <p className='min-w-0'>{
        tracker
          ? <>
            {t('manager.writes.tracker', {
              provider: t(providerLabelKeys[tracker.provider], { ns: 'actionItems' }),
              destination: tracker.destination,
              credential: tracker.credentialName,
            })}
            <span className='ml-2 opacity-60'>{t(trackerSourceLabelKeys[tracker.source])}</span>
          </>
          : <span className='text-warning'>{writes.trackerRefusal ?? t('manager.writes.noTracker')}</span>
      }</p>
      <Button size='sm' variant='secondary' className='shrink-0' onPress={trackerState.open}>
        <span>{t('manager.writes.changeTracker')}</span>
      </Button>
    </div>

    <TrackerModal
      // A tracker saved elsewhere, or by this dialog, opens it afresh on that tracker.
      key={tracker
        ? `${tracker.provider}:${tracker.credentialId}:${tracker.destination}`
        : 'none'}
      state={trackerState}
      projectId={props.manager.projectId}
      tracker={tracker}
    />
  </div>
}
