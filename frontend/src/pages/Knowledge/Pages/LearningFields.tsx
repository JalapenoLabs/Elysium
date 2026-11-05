// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { Markdown } from '../../../components/markdown/Markdown'

// Misc
import { useLearningCategories } from '../../../hooks/useLearningCategories'

type Props = {
  category: string
  evidence: string
}

// What a learning holds beyond its text: the evidence that shows it is true, and where it is filed.
// A category removed since reads as its key, as everywhere else.
export function LearningFields(props: Props) {
  const { t } = useTranslation('knowledge')
  const { labels } = useLearningCategories()

  return <section>
    <div className='compact flex flex-wrap items-center gap-2'>
      <h2 className='text-sm font-semibold'>{t('pages.evidence')}</h2>
      <Chip size='sm' variant='soft'>{labels[props.category] ?? props.category}</Chip>
    </div>
    <Markdown markdown={props.evidence} />
  </section>
}
