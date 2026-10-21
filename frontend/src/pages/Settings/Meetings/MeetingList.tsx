// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllMeetings } from '../../../store/meetingsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { MeetingRow } from './MeetingRow'

// Misc
import { useMeetingsLoader } from '../../../hooks/useServerData'

// The person's recent meetings, newest first, each with how far its processing got.
export function MeetingList() {
  const { t } = useTranslation([ 'meetings', 'common' ])
  const status = useMeetingsLoader()
  const meetings = useAppSelector(selectAllMeetings)

  return <section className='relaxed'>
    <h2 className='compact text-xl font-semibold'>{t('list.heading')}</h2>
    {status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}
    {status === 'failed' && <p className='text-sm text-danger'>{t('common:errors.unexpected')}</p>}
    {status === 'loaded' && !meetings.length && <p
      className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'
    >{
      t('list.empty')
    }</p>}
    {status === 'loaded' && Boolean(meetings.length) && <ul
      className='divide-y divide-separator rounded-xl border border-separator'
    >{
      meetings.map((meeting) => <MeetingRow key={meeting.id} meeting={meeting} />)
    }</ul>}
  </section>
}
