// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { LuEye } from 'react-icons/lu'

// Who can read an environment variable, said plainly wherever variables are managed, since
// "secret" is easy to mistake for "hidden from the agent".
export function EnvironmentVisibilityNote() {
  const { t } = useTranslation('environment')

  return <div className='rounded-xl border border-separator p-4 text-sm'>
    <div className='compact flex items-center gap-2 font-semibold'>
      <LuEye className='size-4 shrink-0' aria-hidden />
      <span>{t('visibility.title')}</span>
    </div>
    <p className='compact opacity-80'>{
      t('visibility.body')
    }</p>
    <p className='opacity-80'>{
      t('visibility.secret')
    }</p>
  </div>
}
