// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../../../store/authSlice'
import { selectCompanyDoc } from '../../../store/companyDocSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Chip, Spinner } from '@heroui/react'
import { LuSparkles } from 'react-icons/lu'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { CompanyDocForm } from './CompanyDocForm'
import { CompanyDocHistory } from './CompanyDocHistory'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { startCompanyDocReview } from '../../../api/routes/companyDocRoutes'
import { useCompanyDocLoader } from '../../../hooks/useServerData'
import { getRunViewUrl } from '../../../urls'
import { toast } from '../../../toast/toast'

// `/settings/company-doc`: the one document that knows the company, edited in place and saved
// from the header, each section with the changes proposed to it beside it, and the doc's
// history below. Look for updates starts the review the engine otherwise runs every few hours.
export function CompanyDocPage() {
  const { t } = useTranslation([ 'companyDoc', 'common' ])
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const status = useCompanyDocLoader(isCompanyMember)
  const doc = useAppSelector(selectCompanyDoc)
  const [ isStarting, setIsStarting ] = useState(false)

  async function review() {
    setIsStarting(true)
    try {
      const response = await startCompanyDocReview()
      const message = response.started
        ? t('toasts.reviewStarted')
        : t('toasts.reviewRunning')
      toast.success(message, {
        link: { label: t('toasts.openRun'), href: getRunViewUrl(response.run.id) },
      })
    }
    catch (error) {
      console.debug('CompanyDocPage could not start a review', { error })
      toast.danger(t('common:errors.unexpected'), { description: getApiErrorMessage(error) ?? undefined })
    }
    finally {
      setIsStarting(false)
    }
  }

  if (!isCompanyMember) {
    return <SettingsPage title={t('title')}>
      <EmptyNotice>{t('membersOnly')}</EmptyNotice>
    </SettingsPage>
  }

  // Until the doc is here the page has no header, as a learning's page has none while it loads,
  // so the header renders once with its actions and Save.
  if (!doc) {
    return status === 'failed'
      ? <p className='container py-10 text-center text-sm text-danger'>{t('loadError')}</p>
      : <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
  }

  let pending = 0
  for (const section of doc.sections) {
    pending += section.proposals.length
  }

  return <SettingsPage
    title={t('title')}
    help={t('page.help')}
    badges={pending > 0 && <Chip size='sm' variant='soft' color='accent'>{
      t('page.pending', { count: pending })
    }</Chip>}
    actions={[{
      id: 'review',
      label: t('page.review'),
      icon: LuSparkles,
      isPending: isStarting,
      onAction: () => void review(),
    }]}
  >
    <div className='relaxed'>
      <CompanyDocForm doc={doc} />
    </div>
    <section className='max-w-3xl'>
      <h2 className='compact text-base font-semibold'>{t('history.title')}</h2>
      <CompanyDocHistory />
    </section>
  </SettingsPage>
}
