// Copyright © 2026 Jalapeno Labs

// The geometry of a drawing over a view. Every point is in the image's own pixels, not the
// screen's, so the drawing flattens onto the full-size image wherever it was drawn.

// What Annotate opens over: the view to draw on and what the drawn prompt records about it.
export type AnnotationSource = {
  // The image itself, or a blob of the model's frozen view.
  imageUrl: string
  // The image, or the model's `.glb`, the drawing is made over.
  sourceAssetId: string
  // model-viewer's camera orbit when a model's view was frozen.
  cameraOrbit?: string
  // The clean frozen view of a model. An image needs none: its asset is the clean view.
  capture?: Blob
}

// x, y, and the pen's pressure from 0 to 1.
export type DrawingPoint = [ number, number, number ]

export const PEN_COLORS = [ 'red', 'yellow', 'blue' ] as const
export type PenColor = typeof PEN_COLORS[number]

export const DRAWING_TOOLS = [ 'pen', 'arrow' ] as const
export type DrawingTool = typeof DRAWING_TOOLS[number]

export type DrawingStroke = {
  tool: DrawingTool
  color: PenColor
  // Every point a pen passed through; an arrow's tail and head.
  points: DrawingPoint[]
  // Drawn with a stylus that reports pressure. A mouse or finger reports none, so the
  // stroke's width follows its speed instead.
  hasPressure: boolean
}

// Theme tokens, read when drawing so the pens match the theme in use.
export const penColorVariables = {
  red: '--danger',
  yellow: '--warning',
  blue: '--accent',
} as const satisfies Record<PenColor, string>

type Rect = {
  left: number
  top: number
  width: number
  height: number
}

// A pointer's place on the screen as a point in the image, which the canvas shows scaled to
// fit its frame.
export function toImagePoint(
  clientX: number,
  clientY: number,
  pressure: number,
  canvasRect: Rect,
  imageWidth: number,
  imageHeight: number,
): DrawingPoint {
  const x = (clientX - canvasRect.left) / canvasRect.width * imageWidth
  const y = (clientY - canvasRect.top) / canvasRect.height * imageHeight
  return [ x, y, pressure ]
}

// A pen's width in image pixels: the same share of the image whatever its resolution, so a
// stroke reads alike over a small render and a large capture.
export function getPenSize(imageWidth: number, imageHeight: number) {
  return Math.max(4, Math.round(Math.min(imageWidth, imageHeight) * 0.008))
}

// The two ends of an arrow's head, each `size` long, swept back from the tip at 30 degrees
// either side of the shaft.
export function getArrowHeadPoints(tail: DrawingPoint, tip: DrawingPoint, size: number) {
  const angle = Math.atan2(tip[1] - tail[1], tip[0] - tail[0])
  const spread = Math.PI / 6
  const left: [ number, number ] = [
    tip[0] - size * Math.cos(angle - spread),
    tip[1] - size * Math.sin(angle - spread),
  ]
  const right: [ number, number ] = [
    tip[0] - size * Math.cos(angle + spread),
    tip[1] - size * Math.sin(angle + spread),
  ]
  return [ left, right ] as const
}

// perfect-freehand's outline of a stroke as SVG path data, filled to draw the stroke. Each
// segment is a quadratic curve through the midpoints, as the library recommends, so the
// outline stays smooth however sparse the pointer's samples were.
export function outlineToPathData(outline: [ number, number ][]) {
  if (!outline.length) {
    return ''
  }

  const [ first ] = outline
  const segments = [ `M ${first[0]} ${first[1]} Q` ]
  for (let index = 0; index < outline.length; index++) {
    const [ x, y ] = outline[index]
    const [ nextX, nextY ] = outline[(index + 1) % outline.length]
    segments.push(`${x} ${y} ${(x + nextX) / 2} ${(y + nextY) / 2}`)
  }
  segments.push('Z')
  return segments.join(' ')
}
