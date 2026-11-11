// Copyright © 2026 Jalapeno Labs

// How every row of the tree looks: a page's, a meeting's, and a project's Meetings group, so they
// read as one tree.

// Hovering fills a row neutrally and the current page is accent-colored, so the two never look
// alike (`~/.claude/docs/ux.md`, Navigation). A row a drag would drop on takes the accent outright,
// and the row being dragged fades, so both read whatever the row was showing.
export const TREE_ROW_CLASS_NAME = [
  'group/page-row relative rounded-md text-sm text-foreground/80 outline-none',
  'transition-colors duration-150 motion-reduce:transition-none',
  'not-data-current:hover:bg-default not-data-current:hover:text-foreground',
  'data-current:bg-accent/15 data-current:font-medium data-current:text-accent',
  'data-focus-visible:ring-2 data-focus-visible:ring-focus data-focus-visible:ring-inset',
  'data-dragging:opacity-50',
  'data-drop-target:bg-accent/30 data-drop-target:ring-2 data-drop-target:ring-accent data-drop-target:ring-inset',
].join(' ')

// A row's content. React Aria names the row's depth; past eight levels a row stops indenting, so a
// deep page's title keeps room in the panel.
export const TREE_ROW_CONTENT_CLASS_NAME = [
  'flex min-w-0 items-center gap-0.5 py-0.5 pr-2',
  'pl-[calc((min(var(--tree-item-level),8)-1)*0.875rem)]',
].join(' ')
