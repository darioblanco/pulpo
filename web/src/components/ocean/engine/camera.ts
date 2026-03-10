export interface Camera {
  /** Center of viewport in world coordinates */
  x: number;
  y: number;
  /** Screen pixels per world unit */
  zoom: number;
  /** Viewport size in screen pixels (CSS pixels, not buffer pixels) */
  width: number;
  height: number;
}

export function createCamera(width: number, height: number): Camera {
  return { x: 0, y: 130, zoom: 2, width, height };
}

export function worldToScreen(cam: Camera, wx: number, wy: number): [number, number] {
  const sx = (wx - cam.x) * cam.zoom + cam.width / 2;
  const sy = (wy - cam.y) * cam.zoom + cam.height / 2;
  return [sx, sy];
}

export function screenToWorld(cam: Camera, sx: number, sy: number): [number, number] {
  const wx = (sx - cam.width / 2) / cam.zoom + cam.x;
  const wy = (sy - cam.height / 2) / cam.zoom + cam.y;
  return [wx, wy];
}

export function fitCamera(cam: Camera, nodes: { x: number }[], padding: number = 100): void {
  if (nodes.length === 0) return;

  const minX = Math.min(...nodes.map((n) => n.x)) - padding;
  const maxX = Math.max(...nodes.map((n) => n.x)) + padding;
  const worldWidth = Math.max(maxX - minX, 200);
  const worldHeight = 280;

  const zoomX = cam.width / worldWidth;
  const zoomY = cam.height / worldHeight;
  cam.zoom = Math.min(zoomX, zoomY, 4);
  cam.x = (minX + maxX) / 2;
  cam.y = worldHeight / 2;
}
