// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { FormEvent } from 'react'
import type { KnowledgePageSummary, PagePlacement } from '../../../api/routes/pageRoutes'
import type { PageTree } from './pageTree'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import {
  Button,
  ComboBox,
  EmptyState,
  Form,
  Input,
  Label,
  ListBox,
  Modal,
  Tooltip,
} from '@heroui/react'
import { OptionSelect } from '../../../components/OptionSelect'

// Misc
import { submitOnEnter } from '../../../components/submitOnEnter'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { findPagePlacement, findSiblingsBesides, listPageDestinations } from './pageTree'

// What the two fields hold for "the top of the tree" and "first", which the API names as null.
const TOP_OF_TREE = 'top'
const FIRST_AMONG_SIBLINGS = 'first'

type Props = {
  page: KnowledgePageSummary
  tree: PageTree
  // Resolves to whether it moved; the dialog stays open to try again until it does.
  onMove: (placement: PagePlacement) => Promise<boolean>
  onClose: () => void
}

// Moves a page from anywhere dragging can't: a touch screen, or a tree too long to drag across. It
// asks for what a move takes, so it can also reorder a page among its siblings. The tree renders it
// only while a page is moving, keyed by that page, so each opens fresh.
export function MovePageModal(props: Props) {
  const { t } = useTranslation('knowledge')
  const { page, tree } = props
  const current = findPagePlacement(tree, page)
  const [ parentId, setParentId ] = useState(current.parentId)
  const [ after, setAfter ] = useState(current.after)
  const [ isMoving, setIsMoving ] = useState(false)
  const inputRef = useInitialFocus()

  const siblings = findSiblingsBesides(tree, parentId, page.id)
  const isUnchanged = parentId === current.parentId && after === current.after

  // Under a new parent a page goes last, where it disturbs nothing, until told otherwise.
  function chooseParent(key: Key | null) {
    if (key === null) {
      console.debug('MovePageModal ignored a cleared destination', { pageId: page.id })
      return
    }
    const nextParentId = key === TOP_OF_TREE
      ? null
      : String(key)
    setParentId(nextParentId)
    if (nextParentId === current.parentId) {
      setAfter(current.after)
      return
    }
    setAfter(findSiblingsBesides(tree, nextParentId, page.id).at(-1)?.id ?? null)
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (isUnchanged || isMoving) {
      return
    }

    setIsMoving(true)
    const isMoved = await props.onMove({ parentId, after })
    setIsMoving(false)
    if (isMoved) {
      props.onClose()
    }
  }

  return <Modal.Backdrop
    isOpen
    onOpenChange={(isOpen) => {
      // Dismissing while the move runs would hide its outcome.
      if (!isOpen && !isMoving) {
        props.onClose()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Form onSubmit={(event) => void submit(event)} onKeyDownCapture={submitOnEnter}>
          <Modal.Header>
            <Modal.Heading>{t('pages.move.title', { title: page.title })}</Modal.Heading>
          </Modal.Header>
          <Modal.Body className='overflow-visible'>
            <div className='compact'>
              <ComboBox
                id='move-page-parent'
                className='w-full'
                selectedKey={parentId ?? TOP_OF_TREE}
                onSelectionChange={chooseParent}
              >
                <Label>{t('pages.move.parent')}</Label>
                <ComboBox.InputGroup>
                  <Input ref={inputRef} />
                  <ComboBox.Trigger />
                </ComboBox.InputGroup>
                <ComboBox.Popover>
                  <ListBox renderEmptyState={() => <EmptyState>{t('pages.move.noResults')}</EmptyState>}>
                    <ListBox.Item id={TOP_OF_TREE} textValue={t('pages.move.top')}>
                      {t('pages.move.top')}
                      <ListBox.ItemIndicator />
                    </ListBox.Item>
                    {listPageDestinations(tree, page.id).map((destination) => <ListBox.Item
                      key={destination.id}
                      id={destination.id}
                      textValue={destination.title}
                    >
                      <div className='min-w-0'>
                        <p className='truncate'>{destination.title}</p>
                        {destination.ancestorTitles.length > 0 && <p className='truncate text-xs opacity-70'>{
                          destination.ancestorTitles.join(' / ')
                        }</p>}
                      </div>
                      <ListBox.ItemIndicator />
                    </ListBox.Item>)}
                  </ListBox>
                </ComboBox.Popover>
              </ComboBox>
            </div>
            <div className='compact'>
              <OptionSelect
                // The siblings under another parent are other options.
                key={parentId ?? TOP_OF_TREE}
                id='move-page-place'
                className='w-full'
                label={t('pages.move.place')}
                options={[
                  { id: FIRST_AMONG_SIBLINGS, label: t('pages.move.first') },
                  ...siblings.map((sibling) => ({
                    id: sibling.id,
                    label: t('pages.move.after', { title: sibling.title }),
                  })),
                ]}
                value={after ?? FIRST_AMONG_SIBLINGS}
                onChange={(value) => setAfter(value === FIRST_AMONG_SIBLINGS
                  ? null
                  : value)}
              />
            </div>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary' isDisabled={isMoving}>
              <span>{t('pages.move.cancel')}</span>
            </Button>
            <Tooltip delay={200} isDisabled={!isUnchanged}>
              <Tooltip.Trigger>
                <div>
                  <Button type='submit' isDisabled={isUnchanged} isPending={isMoving}>
                    <span>{t('pages.move.submit')}</span>
                  </Button>
                </div>
              </Tooltip.Trigger>
              <Tooltip.Content>{t('pages.move.unchanged')}</Tooltip.Content>
            </Tooltip>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
