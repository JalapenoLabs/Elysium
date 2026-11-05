// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { EmptyNotice } from '../../components/EmptyNotice'
import { Page } from '../../layout/page/Page'
import { KnowledgeFrame } from './KnowledgeFrame'

// Misc
import { KNOWLEDGE_CONTENT_CLASS_NAME } from './knowledgeContent'

// Every route of the knowledge base. A guest reaches no knowledge base (docs/knowledge.md, Who sees
// what), so they are told so rather than shown views the API refuses them.
export function KnowledgeLayout() {
  const { t } = useTranslation('knowledge')
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  if (!isCompanyMember) {
    return <div className={KNOWLEDGE_CONTENT_CLASS_NAME}>
      <Page title={t('title')}>
        <EmptyNotice>{t('membersOnly')}</EmptyNotice>
      </Page>
    </div>
  }

  return <KnowledgeFrame />
}
