// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { IssueReportSubject } from '../api/routes/issueReportRoutes'

// Core
import { createContext, useCallback, useState } from 'react'

// User interface
import { useOverlayState } from '@heroui/react'
import { ReportIssueModal } from '../components/ReportIssueModal'

export const ReportIssueContext = createContext<(subject: IssueReportSubject) => void>(() => {
  throw new Error('ReportIssueGate is missing from the component tree')
})

type Props = {
  children: ReactNode
}

type Opened = {
  subject: IssueReportSubject
  // A new one per opening, so the dialog starts empty each time without losing its closing
  // animation to an unmount.
  key: number
}

// One report dialog for the whole app, opened through `useReportIssue` from any surface that
// shows an error. It sits inside the router, so its links are client-side navigations.
export function ReportIssueGate(props: Props) {
  const overlay = useOverlayState()
  const [ opened, setOpened ] = useState<Opened | null>(null)

  const openOverlay = overlay.open
  const report = useCallback((subject: IssueReportSubject) => {
    setOpened((previous) => ({ subject, key: (previous?.key ?? 0) + 1 }))
    openOverlay()
  }, [ openOverlay ])

  return <ReportIssueContext.Provider value={report}>
    {props.children}
    {opened && <ReportIssueModal key={opened.key} state={overlay} subject={opened.subject} />}
  </ReportIssueContext.Provider>
}
