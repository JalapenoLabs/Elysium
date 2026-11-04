// Copyright © 2026 Jalapeno Labs

import type { PresentedLearningCategory } from '../Learnings/learningPresentation'
import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { AlertDialog, Button } from '@heroui/react'
import { CountdownButton } from '../../../components/CountdownButton'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { IRREVERSIBLE_CONFIRM_SECONDS } from '../../../constants'

type Props = {
  // The category to remove. The page renders the dialog only while one is being removed,
  // keyed by it, so each opens fresh.
  category: LearningCategory
  // Every category, as people read them, to choose where its learnings move.
  categories: PresentedLearningCategory[]
  // Removes it, moving its learnings to `moveTo` when it has any. The dialog stays open, its
  // button pending, until this settles, and closes only when it resolves.
  onRemove: (category: LearningCategory, moveTo: string | null) => Promise<void>
  onClose: () => void
}

// Asks before removing one of the workspace's own categories. One that learnings are filed
// under is removed only once the admin chooses where they move, since none may be left without
// a category.
export function RemoveLearningCategoryModal(props: Props) {
  const { t } = useTranslation([ 'learningCategories', 'common' ])
  const category = props.category
  const targets = props.categories.filter((option) => option.key !== category.key)
  const [ moveTo, setMoveTo ] = useState<string | null>(null)
  const [ isPending, setIsPending ] = useState(false)

  const hasLearnings = category.learningCount > 0

  async function remove() {
    setIsPending(true)
    try {
      await props.onRemove(category, hasLearnings
        ? moveTo
        : null)
      props.onClose()
    }
    catch (error) {
      console.debug('RemoveLearningCategoryModal stayed open: the removal failed', { error })
    }
    finally {
      setIsPending(false)
    }
  }

  return <AlertDialog.Backdrop
    isOpen
    // An alert dialog ignores Esc unless told otherwise; leaving this one is always safe.
    isKeyboardDismissDisabled={false}
    onOpenChange={(isOpen) => {
      // Dismissing while the removal runs would hide its outcome.
      if (!isOpen && !isPending) {
        props.onClose()
      }
    }}
  >
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            t('remove.title', { label: category.label })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p className='compact'>
            <span>{t('remove.body', { label: category.label })}</span>
            {' '}
            <strong>{t('common:confirm.irreversible')}</strong>
          </p>
          {hasLearnings && <>
            <p className='compact'>{
              t('remove.learnings', { count: category.learningCount })
            }</p>
            <OptionSelect
              label={t('remove.moveTo')}
              placeholder={t('remove.chooseCategory')}
              options={targets.map((option) => ({ id: option.key, label: option.label }))}
              value={moveTo ?? ''}
              onChange={setMoveTo}
            />
          </>}
        </AlertDialog.Body>
        <AlertDialog.Footer
          onKeyDownCapture={(event) => {
            // A removal is confirmed by a deliberate press, never by Enter (ux.md, Hotkeys).
            if (event.key === 'Enter') {
              event.preventDefault()
              event.stopPropagation()
            }
          }}
        >
          <Button
            variant='tertiary'
            isDisabled={isPending}
            onPress={props.onClose}
          >
            <span>{t('remove.cancel')}</span>
          </Button>
          <CountdownButton
            seconds={IRREVERSIBLE_CONFIRM_SECONDS}
            label={t('remove.confirm')}
            variant='danger'
            isDisabled={hasLearnings && moveTo === null}
            isPending={isPending}
            onPress={() => void remove()}
          />
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
