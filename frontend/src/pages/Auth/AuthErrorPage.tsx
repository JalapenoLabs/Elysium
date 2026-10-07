// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import useSWR from 'swr'

// User interface
import { Link } from '@heroui/react'
import { LuTriangleAlert } from 'react-icons/lu'

// Misc
import { kratos } from '../../api/kratos'
import { UrlTree } from '../../urls'
import { flowErrorText } from './kratosPresentation'

// Kratos sends the browser here when a flow fails in a way no form can show, such as a
// tampered link. The error's own message says what went wrong.
export function AuthErrorPage() {
  const { t } = useTranslation('auth')
  const [ params ] = useSearchParams()
  const errorId = params.get('id')
  const { data } = useSWR(
    errorId
      ? [ 'kratos-error', errorId ]
      : null,
    () => kratos.getFlowError({ id: errorId ?? '' }),
  )
  const explanation = flowErrorText(data?.error)

  return <div className='text-center'>
    <LuTriangleAlert className='relaxed mx-auto size-10 text-warning' aria-hidden />
    <h1 className='text-2xl font-bold'>{t('error.heading')}</h1>
    <p className='relaxed mt-2 text-sm opacity-70'>{
      explanation ?? t('error.description')
    }</p>
    <Link href={UrlTree.login} className='text-link'>{t('error.backToSignIn')}</Link>
  </div>
}
