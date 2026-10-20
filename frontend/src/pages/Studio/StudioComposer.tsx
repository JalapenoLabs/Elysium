// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { StudioItem } from '../../api/routes/studioRoutes'
import type { StudioThreadStatus } from './studioContinuation'

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
import { useSatellitesLoader, useStudioItemLoader } from '../../hooks/useServerData'
import { getStudioThreadStatus } from './studioContinuation'

// The satellite select's key for leaving the choice to the API, which continues on the
// latest session's satellite.
const PREVIOUS_SATELLITE = 'previous'

// The statuses in which a prompt opens a new thread, so a satellite is offered.
type OpeningStatus = Extract<StudioThreadStatus, 'ended' | 'satelliteDeleted' | 'neverRan'>

const noticeKeyByStatus = {
  ended: 'conversation.threadEnded',
  satelliteDeleted: 'conversation.satelliteDeleted',
  neverRan: 'conversation.neverRan',
} as const satisfies Record<OpeningStatus, ParseKeys<'studio'>>

const placeholderKeyByStatus = {
  ended: 'conversation.placeholderContinue',
  satelliteDeleted: 'conversation.placeholderContinue',
  neverRan: 'conversation.placeholderStart',
} as const satisfies Record<OpeningStatus, ParseKeys<'studio'>>

type Props = {
  item: StudioItem
  // The item's newest session; absent until its sessions load, or when it has none.
  latestSession: CodingSession | undefined
}

// Sends the next prompt. Unlike a Coding session's composer, it never closes: a prompt to an
// item without a live thread is how the item continues, in a new thread on the satellite
// chosen here. Only an item whose latest session still has its satellite may leave the choice
// to the API; any other must choose, or the API answers 409.
export function StudioComposer(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  useSatellitesLoader()
  // The item page loads the item too; SWR shares the request. It says whether no session
  // means the item never ran or that its sessions have not arrived yet.
  const itemLoadStatus = useStudioItemLoader(props.item.id)
  const satellites = useAppSelector(selectAllSatellites)
  const [ chosenSatelliteId, setChosenSatelliteId ] = useState<string | null>(null)

  const activeSatellites = useMemo(
    () => satellites.filter((satellite) => satellite.isActive),
    [ satellites ],
  )

  const status = getStudioThreadStatus(props.latestSession, itemLoadStatus !== 'loading')
  const offersPrevious = status === 'ended'
  // A satellite deactivated or deleted since it was chosen is no longer a choice.
  const chosenSatellite = activeSatellites.find((satellite) => satellite.id === chosenSatelliteId)

  async function send(prompt: string) {
    try {
      const response = await sendStudioTurn(props.item.id, {
        prompt,
        // The API ignores a satellite while the latest thread is live.
        satelliteId: chosenSatellite?.id,
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

  if (status === 'loading') {
    return <PromptComposer
      onSend={send}
      isClosed={false}
      sendBlocker={t('conversation.loading')}
    />
  }

  if (status === 'live') {
    return <PromptComposer
      onSend={send}
      isClosed={false}
    />
  }

  const selectedKey = chosenSatellite?.id ?? (offersPrevious
    ? PREVIOUS_SATELLITE
    : null)
  const hasChoices = offersPrevious || activeSatellites.length > 0
  let sendBlocker: string | undefined
  if (!hasChoices) {
    sendBlocker = t('conversation.noActiveSatellite')
  }
  else if (!selectedKey) {
    sendBlocker = t('conversation.satelliteRequired')
  }

  // The notice and the satellite share a row above the field, and the satellite wraps
  // under the notice when the column is too narrow for both.
  const header = <div className='flex flex-wrap items-center gap-x-3 gap-y-2'>
    <p className='min-w-48 flex-1 text-xs opacity-70'>{
      t(noticeKeyByStatus[status])
    }</p>
    <Select
      className='w-56 max-w-full'
      aria-label={t('conversation.satellite')}
      placeholder={t('conversation.satellitePlaceholder')}
      isDisabled={!hasChoices}
      value={selectedKey}
      onChange={(key) => {
        const satelliteId = key === null || key === PREVIOUS_SATELLITE
          ? null
          : String(key)
        setChosenSatelliteId(satelliteId)
      }}
    >
      <Label className='sr-only'>{t('conversation.satellite')}</Label>
      <Select.Trigger>
        <Select.Value />
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          {offersPrevious && <ListBox.Item id={PREVIOUS_SATELLITE} textValue={t('conversation.previousSatellite')}>
            {t('conversation.previousSatellite')}
            <ListBox.ItemIndicator />
          </ListBox.Item>}
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
  </div>

  return <PromptComposer
    onSend={send}
    isClosed={false}
    header={header}
    placeholder={t(placeholderKeyByStatus[status])}
    sendBlocker={sendBlocker}
  />
}
