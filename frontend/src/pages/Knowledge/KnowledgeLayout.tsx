// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Outlet, useMatch } from 'react-router'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { EmptyNotice } from '../../components/EmptyNotice'
import { Page } from '../../layout/page/Page'
import { KnowledgeFrame } from './KnowledgeFrame'

// Misc
import { UrlTree } from '../../urls'
import { KNOWLEDGE_CONTENT_CLASS_NAME } from './knowledgeContent'

// Every route of the knowledge base. A guest reaches no knowledge base (docs/knowledge.md, Who sees
// what), so they are told so rather than shown views the API refuses them, but for a meeting shared
// with them, which they read at its own address, alone, without the knowledge base around it
// (docs/meeting-review.md, Who sees a meeting).
export function KnowledgeLayout() {
  const { t } = useTranslation('knowledge')
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const isMeetingPage = Boolean(useMatch(UrlTree.knowledgeMeeting))

  if (!isCompanyMember && isMeetingPage) {
    return <div className={KNOWLEDGE_CONTENT_CLASS_NAME}>
      <Outlet />
    </div>
  }
  if (!isCompanyMember) {
    return <div className={KNOWLEDGE_CONTENT_CLASS_NAME}>
      <Page title={t('title')}>
        <EmptyNotice>{t('membersOnly')}</EmptyNotice>
      </Page>
    </div>
  }

  return <KnowledgeFrame />
}
