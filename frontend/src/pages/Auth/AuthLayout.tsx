// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Outlet, useHref, useNavigate } from 'react-router'

// User interface
import { RouterProvider as AriaRouterProvider, Toast } from '@heroui/react'

// The frame around the pages people see before they may use the workspace: signing in,
// signing up, recovery, and waiting for approval. One centered card under the brand.
export function AuthLayout() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  return <AriaRouterProvider navigate={navigate} useHref={useHref}>
    <main className='flex min-h-dvh flex-col items-center justify-center bg-surface-secondary px-4 py-12'>
      <div className='level-left relaxed gap-2'>
        <div className='grid size-9 place-items-center rounded-lg bg-accent text-sm font-bold text-accent-foreground'>
          E
        </div>
        <span className='text-lg font-semibold'>{
          t('brand.name')
        }</span>
      </div>
      <div className='w-full max-w-md rounded-2xl border border-separator bg-surface p-8 shadow-sm'>
        <Outlet />
      </div>
    </main>
    <Toast.Provider placement='bottom end' />
  </AriaRouterProvider>
}
