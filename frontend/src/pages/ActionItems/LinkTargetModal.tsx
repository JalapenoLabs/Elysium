// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { LinkTarget } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form, Modal } from '@heroui/react'
import { LinkTargetPicker } from './LinkTargetPicker'

// Misc
import { submitOnEnter } from '../../components/submitOnEnter'
import { EMPTY_LINK_TARGET_DRAFT, toLinkTarget } from './linkPresentation'

type Props = {
  state: UseOverlayStateReturn
  title: string
  submitLabel: string
  // Throws to keep the dialog open on what was picked, after reporting why.
  onSubmit: (target: LinkTarget) => Promise<void>
}

// A dialog that picks one Jira issue, GitHub issue, or pull request, for linking it to an
// item or for creating an item from it.
export function LinkTargetModal(props: Props) {
  const { t } = useTranslation('common')
  const [ draft, setDraft ] = useState(EMPTY_LINK_TARGET_DRAFT)
  const [ isSaving, setIsSaving ] = useState(false)
  const target = toLinkTarget(draft)

  function close() {
    // The next opening starts over.
    setDraft(EMPTY_LINK_TARGET_DRAFT)
    props.state.close()
  }

  async function submit() {
    if (!target || isSaving) {
      console.debug('LinkTargetModal ignored a submit', { hasTarget: Boolean(target), isSaving })
      return
    }
    setIsSaving(true)
    try {
      await props.onSubmit(target)
      close()
    }
    catch (error) {
      // The caller reported it; the dialog stays open to pick again.
      console.debug('LinkTargetModal kept the dialog open after a failed submit', { error })
    }
    finally {
      setIsSaving(false)
    }
  }

  // Controlled overlays skip the Modal root, as in SnoozeDateModal.
  return <Modal.Backdrop
    isOpen={props.state.isOpen}
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        close()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{props.title}</Modal.Heading>
        </Modal.Header>
        {/* Enter links what is picked (ux.md, Hotkeys) */}
        <Form
          validationBehavior='aria'
          onKeyDownCapture={submitOnEnter}
          onSubmit={(event) => {
            event.preventDefault()
            void submit()
          }}
        >
          <Modal.Body className='mt-2'>
            <LinkTargetPicker value={draft} onChange={setDraft} />
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!target} isPending={isSaving}>
              <span>{props.submitLabel}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
