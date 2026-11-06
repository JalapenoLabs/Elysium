// Copyright © 2026 Jalapeno Labs

import type { Editor, JSONContent } from '@tiptap/core'

// Misc
import {
  findAttachmentPlaceholder,
  placeAttachmentPlaceholder,
  removeAttachmentPlaceholder,
} from './attachmentPlaceholders'

// A file a page holds, as the editor refers to it.
export type AttachedFile = {
  name: string
  url: string
  isImage: boolean
}

// What a file attached here becomes in the page: Markdown, as everything in it is. An image displays
// (`![name](address)`), and any other file is a link (`[name](address)`).
function describeFile(file: AttachedFile): JSONContent {
  if (file.isImage) {
    const dot = file.name.lastIndexOf('.')
    return {
      type: 'image',
      attrs: {
        src: file.url,
        alt: dot > 0
          ? file.name.slice(0, dot)
          : file.name,
      },
    }
  }
  return {
    type: 'text',
    text: file.name,
    marks: [{ type: 'link', attrs: { href: file.url }}],
  }
}

// `position`, or the place after its block for code, which holds neither an image nor a link.
function placeFor(editor: Editor, position: number) {
  const resolved = editor.state.doc.resolve(position)
  return resolved.parent.type.spec.code
    ? resolved.after()
    : position
}

type AttachOptions = {
  attach: (file: File) => Promise<AttachedFile | null>
  uploadingLabel: (name: string) => string
}

// Uploads `files` one after another and puts each into the page where it was dropped, in order.
// Each holds its place with a placeholder until it arrives, and `attach` says why when one does not,
// which removes only its own placeholder. A file whose place was deleted meanwhile stays attached
// and listed on the page, and nothing is inserted.
export async function attachEditorFiles(
  editor: Editor,
  files: File[],
  position: number,
  options: AttachOptions,
) {
  const place = placeFor(editor, position)
  const placed = files.map((file) => {
    const id = {}
    placeAttachmentPlaceholder(editor.view, id, place, options.uploadingLabel(file.name))
    return { file, id }
  })

  for (const [ index, { file, id }] of placed.entries()) {
    const attached = await options.attach(file)
    if (editor.isDestroyed) {
      console.debug('attachEditorFiles finished after its editor closed', { name: file.name })
      continue
    }

    const landing = findAttachmentPlaceholder(editor.state, id)
    removeAttachmentPlaceholder(editor.view, id)
    if (!attached || landing === null) {
      console.debug('attachEditorFiles inserted nothing for a file', { name: file.name, landing })
      continue
    }

    const content: JSONContent[] = [ describeFile(attached) ]
    // Links side by side would read as one, and the next file lands right after this one.
    if (index < placed.length - 1) {
      content.push({ type: 'text', text: ' ' })
    }
    editor.chain().insertContentAt(landing, content, { updateSelection: false }).run()
  }
}
