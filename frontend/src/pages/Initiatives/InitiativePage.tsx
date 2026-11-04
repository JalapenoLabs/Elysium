// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInitiativeById } from '../../store/initiativesSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { InitiativeView } from './InitiativeView'

// Misc
import { useInitiativeLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

// `/action-items/initiatives/:initiativeId`: loads the initiative, then hands it to
// `InitiativeView`, which can rely on it being there.
export function InitiativePage() {
  const { t } = useTranslation('initiatives')
  const { initiativeId = '' } = useParams()
  const status = useInitiativeLoader(initiativeId)
  const initiative = useAppSelector((state) => selectInitiativeById(state, initiativeId))

  if (initiative) {
    return <InitiativeView initiative={initiative} />
  }

  return <div className='container'>{
    status === 'loading'
      ? <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
      : <div className='py-16 text-center'>
        <p className='compact text-sm opacity-70'>{t('page.notFound')}</p>
        <Link href={UrlTree.initiatives} className='text-link'>{t('page.backToList')}</Link>
      </div>
  }</div>
}
