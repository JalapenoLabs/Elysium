// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Outlet, useHref, useNavigate } from 'react-router'

// User interface
import { RouterProvider as AriaRouterProvider, Toast } from '@heroui/react'
import { AuthSplash } from './AuthSplash'

// The frame around the pages people see before they may use the workspace: signing in,
// signing up, recovery, and waiting for approval. One centered card under the brand, over an
// animated splash.
export function AuthLayout() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  return <AriaRouterProvider navigate={navigate} useHref={useHref}>
    <main className='relative isolate flex min-h-dvh flex-col items-center justify-center overflow-hidden px-4 py-12'>
      <AuthSplash />
      <div className='auth-splash-rise relative flex w-full max-w-md flex-col items-center'>
        <div className='level-left relaxed gap-2'>
          <div className='auth-splash-mark grid size-9 place-items-center rounded-lg text-sm font-bold text-white'>
            E
          </div>
          <span className='text-lg font-semibold text-white'>{
            t('brand.name')
          }</span>
        </div>
        <div className='auth-splash-card w-full rounded-2xl p-8'>
          <Outlet />
        </div>
      </div>
    </main>
    <Toast.Provider placement='bottom end' />
  </AriaRouterProvider>
}
