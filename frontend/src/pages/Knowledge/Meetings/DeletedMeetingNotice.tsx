// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../../../api/routes/meetingRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { DateText } from '../../../format/DateText'
import { useMeetingDeletion } from './useMeetingDeletion'

type Props = {
  // A deleted meeting.
  meeting: Meeting
}

// What a deleted meeting says on its page: when it was deleted, and Restore, which brings it back
// with its excerpts and carries on the run its deletion stopped (docs/meetings.md, Deleting). The
// page reads as the meeting stood, changing nothing until then.
export function DeletedMeetingNotice(props: Props) {
  const { t } = useTranslation('meetings')
  const deletion = useMeetingDeletion()
  const [ isRestoring, setIsRestoring ] = useState(false)

  async function restore() {
    setIsRestoring(true)
    try {
      await deletion.restore([ props.meeting ])
    }
    finally {
      setIsRestoring(false)
    }
  }

  // The way back sits under what it explains, so a phone gives the words the notice's whole width.
  return <Alert status='danger' className='relaxed'>
    <Alert.Indicator />
    <Alert.Content>
      <Alert.Title>{t('delete.deletedTitle')}</Alert.Title>
      {props.meeting.deletedAt && <Alert.Description>
        <span>{t('delete.deletedOn')}</span>
        {' '}
        <DateText value={props.meeting.deletedAt} />
      </Alert.Description>}
      <Button
        size='sm'
        variant='outline'
        className='mt-2 self-start'
        isPending={isRestoring}
        isDisabled={isRestoring}
        onPress={() => void restore()}
      >
        <LuUndo2 className='size-4' aria-hidden />
        <span>{t('delete.restore')}</span>
      </Button>
    </Alert.Content>
  </Alert>
}
