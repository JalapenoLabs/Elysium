// Copyright © 2026 Jalapeno Labs

import type { DrawingStroke } from './annotation'

// Lib
import { getStroke } from 'perfect-freehand'

// Misc
import { getArrowHeadPoints, outlineToPathData, penColorVariables } from './annotation'

// Paints the view and every stroke over it at the image's full size, so the canvas itself
// is the flattened drawing the agent receives. Redrawn whole on every change; a drawing holds
// few enough strokes that this stays cheap.
export function drawAnnotation(
  context: CanvasRenderingContext2D,
  image: HTMLImageElement,
  strokes: DrawingStroke[],
  penSize: number,
) {
  const { width, height } = context.canvas
  context.clearRect(0, 0, width, height)
  context.drawImage(image, 0, 0, width, height)

  // The pens are theme colors, read now so a drawing follows the theme in use.
  const styles = window.getComputedStyle(document.documentElement)

  for (const stroke of strokes) {
    const color = styles.getPropertyValue(penColorVariables[stroke.color]).trim()
    context.fillStyle = color
    context.strokeStyle = color

    if (stroke.tool === 'pen') {
      const outline = getStroke(stroke.points, {
        size: penSize,
        thinning: 0.5,
        smoothing: 0.5,
        streamline: 0.5,
        simulatePressure: !stroke.hasPressure,
      })
      context.fill(new Path2D(outlineToPathData(outline)))
      continue
    }

    // An arrow shows once the pointer has moved from where it went down.
    const [ tail, tip ] = [ stroke.points[0], stroke.points.at(-1) ]
    if (stroke.points.length < 2 || !tail || !tip) {
      continue
    }
    const [ left, right ] = getArrowHeadPoints(tail, tip, penSize * 4)
    context.lineWidth = penSize
    context.lineCap = 'round'
    context.lineJoin = 'round'
    context.beginPath()
    context.moveTo(tail[0], tail[1])
    context.lineTo(tip[0], tip[1])
    context.moveTo(left[0], left[1])
    context.lineTo(tip[0], tip[1])
    context.lineTo(right[0], right[1])
    context.stroke()
  }
}
