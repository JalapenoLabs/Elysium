// Copyright © 2026 Jalapeno Labs

import type { AuditEntry } from '../../api/routes/orgRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Card, CardBody } from '@heroui/react'

// Utility
import moment from 'moment-timezone'

// Misc
import { getOrgAuditLog } from '../../api/routes/orgRoutes'

// The organization's audit trail: who did what, most recent first. Admins and
// members with `view_audit` can see it (the server enforces access; the sidebar
// entry is always present, and an unauthorized load simply shows the error).
export function OrgAuditLog() {
  const { t } = useTranslation('auth')

  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)

  const [ entries, setEntries ] = useState<AuditEntry[]>([])
  const [ isLoading, setIsLoading ] = useState(true)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  useEffect(() => {
    if (!activeOrgId) {
      return undefined
    }
    let cancelled = false
    setIsLoading(true)
    getOrgAuditLog(activeOrgId)
      .then((response) => {
        if (!cancelled) {
          setEntries(response.entries)
        }
      })
      .catch((error) => {
        console.debug('audit log unavailable', { error })
        if (!cancelled) {
          setErrorMessage(t('orgAudit.loadFailed'))
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [ activeOrgId, t ])

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('orgAudit.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('orgAudit.subtitle') }</p>
    </div>

    { errorMessage
      ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
      : isLoading
        ? <p className='opacity-60'>{ t('orgAudit.loading') }</p>
        : entries.length
          ? entries.map((entry, index) =>
            <div key={index} className='compact'>
              <Card className='p-1'>
                <CardBody className='level items-center'>
                  <div>
                    {/* The action + actor is the headline; the target and any
                        detail refine it. Action names come from the server as
                        machine codes, translated here with a raw fallback. */}
                    <p>
                      <strong>{ entry.actor.display_name || entry.actor.email }</strong>
                      <span className='opacity-80'>
                        { ' ' }{ t(`orgAudit.actions.${entry.action}`, entry.action) }
                      </span>
                      { entry.target?.display_name || entry.target?.email
                        ? <strong>{ ' ' }{ entry.target.display_name || entry.target.email }</strong>
                        : null }
                    </p>
                    { entry.detail
                      ? <p className='text-xs opacity-60'>{ entry.detail }</p>
                      : null }
                  </div>
                  <span className='text-sm opacity-70'>
                    { moment(entry.created_at).format('MMM D, h:mm A') }
                  </span>
                </CardBody>
              </Card>
            </div>,
          )
          : <p className='opacity-60'>{ t('orgAudit.empty') }</p> }
  </section>
}
