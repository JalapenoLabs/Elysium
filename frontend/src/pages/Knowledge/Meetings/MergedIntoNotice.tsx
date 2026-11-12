// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectMeetingById } from '../../../store/meetingsSlice'

// User interface
import { Alert, buttonVariants, Link } from '@heroui/react'

// Misc
import { useMeetingLoader } from '../../../hooks/useServerData'
import { getMeetingUrl } from '../../../urls'

type Props = {
  // The meeting this one's captures were merged into.
  mergedInto: string
}

// What a meeting merged into another says on its page: its captures live in the other now, which
// is where they are split back out (docs/meetings.md, The same meeting twice). Its history still
// records the merge, and its page stays its own.
export function MergedIntoNotice(props: Props) {
  const { t } = useTranslation('meetings')
  // The meeting kept is one of the person's listed meetings, so loading it names it here.
  useMeetingLoader(props.mergedInto)
  const kept = useAppSelector((state) => selectMeetingById(state, props.mergedInto))

  // The way there sits under what it explains, so a phone gives the words the notice's whole width.
  return <Alert status='accent' className='relaxed'>
    <Alert.Indicator />
    <Alert.Content>
      <Alert.Title>{t('page.mergedTitle')}</Alert.Title>
      <Alert.Description>{
        kept?.title
          ? t('page.mergedInto', { title: kept.title })
          : t('page.mergedIntoUntitled')
      }</Alert.Description>
      <Link
        href={getMeetingUrl(props.mergedInto)}
        className={buttonVariants({ size: 'sm', variant: 'outline', className: 'mt-2 self-start' })}
      >
        <span>{t('page.openMergedInto')}</span>
      </Link>
    </Alert.Content>
  </Alert>
}
