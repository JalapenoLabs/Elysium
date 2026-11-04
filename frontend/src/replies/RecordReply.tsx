// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../api/routes/timelineRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'
import { LuReply } from 'react-icons/lu'
import { ReplyComposer } from './ReplyComposer'

// Misc
import { useRecordReplies } from './useRecordReplies'

type Props = {
  record: TimelineRecord
}

// Reply, for a record whose kind can be answered, behind a button where records are listed.
// Records nothing answers show nothing.
export function RecordReply(props: Props) {
  const { t } = useTranslation('timeline')
  const replies = useRecordReplies(props.record)
  const [ isOpened, setIsOpened ] = useState(false)

  if (!replies) {
    return null
  }

  if (!isOpened) {
    return <Button size='sm' variant='ghost' onPress={() => setIsOpened(true)}>
      <LuReply className='size-4' aria-hidden />
      <span>{t('reply.open')}</span>
    </Button>
  }

  return <ReplyComposer record={props.record} replies={replies} />
}
