// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectCodingSessionsByActionItemId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Spinner } from '@heroui/react'
import { CodingSessionsTable } from '../Coding/CodingSessionsTable'

// Misc
import { useCodingSessionsLoader } from '../../hooks/useServerData'

type Props = {
  itemId: string
}

// The coding sessions started from an item, newest first, each opening its conversation.
export function ActionItemSessions(props: Props) {
  const { t } = useTranslation('actionItems')
  const status = useCodingSessionsLoader()
  const sessions = useAppSelector((state) => selectCodingSessionsByActionItemId(state, props.itemId))

  if (status === 'loading') {
    return <div className='grid place-items-center py-6'>
      <Spinner />
    </div>
  }

  return <CodingSessionsTable
    sessions={sessions}
    ids={{
      tableElementId: 'action-item-sessions-table',
      tableLocalStorageId: 'elysium.actionItems.sessions.table.v1',
    }}
    ariaLabel={t('sessions.heading')}
    emptyMessage={t('sessions.empty')}
  />
}
