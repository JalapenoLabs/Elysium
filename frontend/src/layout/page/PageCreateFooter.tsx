// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { PageSaveButton } from './PageSaveButton'

// Misc
import { PageSaveStateContext } from './pageSave'

// The bottom of a /new page's form (`~/.claude/docs/ux.md`, Form layout): Back on the left,
// a real link to where the page's back button goes, and on the right the page's Save under the
// same label, its second place. Both run one save, so the header's button, this one, Ctrl+S,
// and Enter can never disagree about what creating means or whether it can happen yet. With
// unsaved work, Back asks first, as every other way out does. See docs/frontend.md, Page header.
export function PageCreateFooter() {
  const { t } = useTranslation('common')
  const state = useContext(PageSaveStateContext)
  if (!state) {
    throw new Error('PageCreateFooter must be rendered inside a Page')
  }

  return <div ref={state.createFooterRef} className='page-create-footer'>
    {state.backTo && <Link
      href={state.backTo}
      className={buttonVariants({ variant: 'outline' })}
    >
      <LuArrowLeft className='size-4' aria-hidden />
      <span>{t('actions.back')}</span>
    </Link>}
    {state.save && <PageSaveButton
      save={state.save}
      isSaving={state.isSaving}
      onPress={() => void state.runSave()}
    />}
  </div>
}
