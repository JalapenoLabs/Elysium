// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { AlertDialog, Button, Tooltip } from '@heroui/react'

// Misc
import { useInitialFocus } from '../../hooks/useInitialFocus'

type Props = {
  isOpen: boolean
  // Save and continue is running: every choice waits for it.
  isSaving: boolean
  // Why the changes cannot be saved as they are, such as a required field left empty. Save
  // and continue is disabled and says so while set.
  saveBlockedReason: string | null
  onDiscard: () => void
  onSaveAndContinue: () => void
  onKeepEditing: () => void
}

// Asked when leaving a page with unsaved changes, however the person leaves: a link, the
// sidebar, the breadcrumbs, Esc, or the browser's Back (`~/.claude/docs/ux.md`, Saving).
// Enter saves and continues; Esc keeps editing.
export function LeavePageDialog(props: Props) {
  const { t } = useTranslation('common')
  const canSave = !props.saveBlockedReason
  // Focused, Enter saves and continues, as in any dialog (ux.md, Hotkeys); with nothing it
  // could save, Enter keeps editing.
  const saveRef = useInitialFocus(canSave)
  const keepEditingRef = useInitialFocus(!canSave)

  const saveButton = <Button
    ref={saveRef}
    variant='primary'
    isDisabled={!canSave}
    isPending={props.isSaving}
    onPress={props.onSaveAndContinue}
  >
    <span>{t('page.leave.saveAndContinue')}</span>
  </Button>

  return <AlertDialog.Backdrop
    isOpen={props.isOpen}
    onOpenChange={(isOpen) => {
      // Dismissing while saving would hide whether the page could be left.
      if (!isOpen && !props.isSaving) {
        props.onKeepEditing()
      }
    }}
    // An alert dialog ignores Esc unless told otherwise; Esc here is Keep editing.
    isKeyboardDismissDisabled={false}
  >
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='warning' />
          <AlertDialog.Heading>{t('page.leave.title')}</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{t('page.leave.message')}</p>
        </AlertDialog.Body>
        <AlertDialog.Footer>
          {/* Furthest from the primary button, so it is never pressed by a slip. */}
          <Button
            variant='danger-soft'
            className='sm:mr-auto'
            isDisabled={props.isSaving}
            onPress={props.onDiscard}
          >
            <span>{t('page.leave.discard')}</span>
          </Button>
          <Button
            ref={keepEditingRef}
            variant='tertiary'
            isDisabled={props.isSaving}
            onPress={props.onKeepEditing}
          >
            <span>{t('page.leave.keepEditing')}</span>
          </Button>
          {canSave
            ? saveButton
            : <Tooltip delay={200}>
              <Tooltip.Trigger>
                <div>{saveButton}</div>
              </Tooltip.Trigger>
              <Tooltip.Content className='max-w-xs'>
                <span>{props.saveBlockedReason}</span>
              </Tooltip.Content>
            </Tooltip>}
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}
