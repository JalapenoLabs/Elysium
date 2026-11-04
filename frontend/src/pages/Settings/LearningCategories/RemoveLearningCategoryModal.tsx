// Copyright © 2026 Jalapeno Labs

import type { PresentedLearningCategory } from '../Learnings/learningPresentation'
import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { AlertDialog, Button } from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// A removal cannot be undone, so its button waits this many seconds before it can be pressed.
const CONFIRM_DELAY_SECONDS = 5

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
  const { t } = useTranslation('learningCategories')
  const category = props.category
  const targets = props.categories.filter((option) => option.key !== category.key)
  const [ moveTo, setMoveTo ] = useState<string | null>(null)
  const [ secondsLeft, setSecondsLeft ] = useState(CONFIRM_DELAY_SECONDS)
  const [ isPending, setIsPending ] = useState(false)

  useEffect(() => {
    const timer = window.setInterval(() => {
      setSecondsLeft((seconds) => Math.max(seconds - 1, 0))
    }, 1000)
    return () => window.clearInterval(timer)
  }, [])

  const hasLearnings = category.learningCount > 0
  const canRemove = secondsLeft === 0 && (!hasLearnings || moveTo !== null)

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
            <strong>{t('remove.irreversible')}</strong>
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
        <AlertDialog.Footer>
          <Button
            variant='tertiary'
            isDisabled={isPending}
            onPress={props.onClose}
          >
            <span>{t('remove.cancel')}</span>
          </Button>
          <Button
            variant='danger'
            isDisabled={!canRemove}
            isPending={isPending}
            onPress={() => void remove()}
          >
            <span>{
              secondsLeft > 0
                ? t('remove.confirmIn', { seconds: secondsLeft })
                : t('remove.confirm')
            }</span>
          </Button>
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
