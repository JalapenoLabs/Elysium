// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectTimelineRecordByActionItemId } from '../store/timelineRecordsSlice'

// User interface
import { ReplyComposer } from './ReplyComposer'

// Misc
import { useActionItemRecordLoader } from '../hooks/useServerData'
import { useRecordReplies } from './useRecordReplies'

type Props = {
  itemId: string
}

// The reply box of an item a message raised, such as a Slack mention or an unread mail, so the
// person answers it where they triage it. An item no message raised, or one whose kind nothing
// answers, shows nothing.
export function ActionItemReply(props: Props) {
  const { t } = useTranslation('timeline')
  useActionItemRecordLoader(props.itemId)
  const record = useAppSelector((state) => selectTimelineRecordByActionItemId(state, props.itemId))
  const replies = useRecordReplies(record)

  if (!record || !replies) {
    return null
  }

  return <section className='relaxed'>
    <h2 className='compact text-lg font-semibold'>{t('reply.heading')}</h2>
    <ReplyComposer record={record} replies={replies} />
  </section>
}
