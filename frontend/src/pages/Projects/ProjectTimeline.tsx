// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectTimelineRecordsByProjectId } from '../../store/timelineRecordsSlice'

// User interface
import { TimelinePageLoader } from './TimelinePageLoader'
import { TimelineRecordItem } from './TimelineRecordItem'

// Misc
import { useProjectTimelineLoader } from '../../hooks/useServerData'

type Props = {
  projectId: string
}

// Everything about a project, newest first: mail, meeting segments with their clips, and
// whatever else is attributed to it, by the person or by Elysia. See docs/connectors.md.
export function ProjectTimeline(props: Props) {
  const { t } = useTranslation('timeline')
  // The newest page, which the loader below also asks for; SWR shares the one request.
  const newest = useProjectTimelineLoader(props.projectId, null)
  const records = useAppSelector((state) => selectTimelineRecordsByProjectId(state, props.projectId))

  return <section className='relaxed'>
    <h2 className='text-xl font-semibold'>{t('heading')}</h2>
    <p className='compact text-sm opacity-70'>{t('description')}</p>
    {Boolean(records.length) && <ol className='relative flex flex-col gap-3 border-l border-separator pl-5'>{
      records.map((record) => <TimelineRecordItem
        key={record.id}
        record={record}
        projectId={props.projectId}
      />)
    }</ol>}
    <TimelinePageLoader projectId={props.projectId} before={null} />
    {newest.status === 'loaded' && !records.length && <p
      className='rounded-xl border border-separator py-8 text-center text-sm opacity-70'
    >{
      t('empty')
    }</p>}
  </section>
}
