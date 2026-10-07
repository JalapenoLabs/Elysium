// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { selectAllSatellites } from '../../store/satellitesSlice'
import { studioFeedbackCreated } from '../../store/studioFeedbackSlice'

// User interface
import { Label, ListBox, Select, toast } from '@heroui/react'
import { PromptComposer } from '../Coding/PromptComposer'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { sendStudioTurn } from '../../api/routes/studioRoutes'
import { useSatellitesLoader } from '../../hooks/useServerData'
import { CLOSED_THREAD_STATES } from '../Coding/sessionPresentation'

// The satellite select's key for leaving the choice to the API, which continues on the
// previous satellite.
const PREVIOUS_SATELLITE = 'previous'

type Props = {
  item: StudioItem
  // The item's newest session; absent until its sessions load.
  latestSession: CodingSession | undefined
}

// Sends the next prompt. Unlike a Coding session's composer, it never closes: a prompt to an
// item whose thread has ended is how the item continues, in a new thread on the satellite
// chosen here or else the previous one.
export function StudioComposer(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  useSatellitesLoader()
  const satellites = useAppSelector(selectAllSatellites)
  const [ satelliteChoice, setSatelliteChoice ] = useState(PREVIOUS_SATELLITE)

  const activeSatellites = useMemo(
    () => satellites.filter((satellite) => satellite.isActive),
    [ satellites ],
  )

  const state = props.latestSession?.thread?.state
  const hasEnded = !props.latestSession
    || props.latestSession.satelliteId === null
    || (state !== undefined && CLOSED_THREAD_STATES.includes(state))

  async function send(prompt: string) {
    try {
      const response = await sendStudioTurn(props.item.id, {
        prompt,
        satelliteId: hasEnded && satelliteChoice !== PREVIOUS_SATELLITE
          ? satelliteChoice
          : undefined,
      })
      dispatch(codingSessionUpserted(response.session))
      if (response.feedback) {
        dispatch(studioFeedbackCreated(response.feedback))
      }
      return true
    }
    catch (error) {
      // A continuation with no usable satellite answers 409, and a satellite that refused the
      // thread 502; both say what went wrong.
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('StudioComposer failed to send a prompt', { error, itemId: props.item.id })
      }
      toast.danger(t('toasts.sendFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  }

  const satelliteSelect = <Select
    className='w-44'
    aria-label={t('conversation.satellite')}
    value={satelliteChoice}
    onChange={(key) => setSatelliteChoice(String(key ?? PREVIOUS_SATELLITE))}
  >
    <Label className='sr-only'>{t('conversation.satellite')}</Label>
    <Select.Trigger>
      <Select.Value />
      <Select.Indicator />
    </Select.Trigger>
    <Select.Popover>
      <ListBox>
        <ListBox.Item id={PREVIOUS_SATELLITE} textValue={t('conversation.satellitePlaceholder')}>
          {t('conversation.satellitePlaceholder')}
          <ListBox.ItemIndicator />
        </ListBox.Item>
        {activeSatellites.map((satellite) => <ListBox.Item
          key={satellite.id}
          id={satellite.id}
          textValue={satellite.name}
        >
          {satellite.name}
          <ListBox.ItemIndicator />
        </ListBox.Item>)}
      </ListBox>
    </Select.Popover>
  </Select>

  return <div className='shrink-0'>
    {hasEnded && <p className='px-4 pb-2 text-center text-xs opacity-70'>{
      t('conversation.threadEnded')
    }</p>}
    <PromptComposer
      onSend={send}
      isClosed={false}
      accessory={hasEnded
        ? satelliteSelect
        : undefined}
      placeholder={hasEnded
        ? t('conversation.placeholderContinue')
        : undefined}
    />
  </div>
}
