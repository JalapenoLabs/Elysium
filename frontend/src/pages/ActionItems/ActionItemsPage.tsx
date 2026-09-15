// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// A placeholder until the first action items feature lands.
export function ActionItemsPage() {
  const { t } = useTranslation('actionItems')

  return <section className='container'>
    <h1 className='compact text-2xl font-semibold'>{
      t('title')
    }</h1>
    <p className='opacity-70'>{
      t('subtitle')
    }</p>
  </section>
}
