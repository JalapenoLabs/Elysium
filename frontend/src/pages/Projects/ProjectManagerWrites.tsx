// Copyright © 2026 Jalapeno Labs

import type { ProjectManager } from '../../api/routes/projectManagerRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, useOverlayState } from '@heroui/react'
import { HelpPopover } from '../../components/HelpPopover'
import { TrackerModal } from './TrackerModal'

// Misc
import { providerLabelKeys } from '../ActionItems/linkPresentation'
import { trackerSourceLabelKeys } from './trackerPresentation'

type Props = {
  manager: ProjectManager
}

// How the project's Project Manager writes: whether its changes apply as it makes them or wait
// for a person's approval, which the project's level of autonomy on this page decides, and the
// tracker its issues file in, with a way to change it.
export function ProjectManagerWrites(props: Props) {
  const { t } = useTranslation([ 'projects', 'actionItems' ])
  const trackerState = useOverlayState()
  const writes = props.manager.writes
  const tracker = writes.tracker

  return <div className='relaxed flex flex-col gap-2 rounded-xl border border-separator p-4 text-sm'>
    <div className='flex items-center gap-1'>
      <p>{
        writes.direct
          ? t('manager.writes.direct')
          : t('manager.writes.reviewed')
      }</p>
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
