import type { Sprites, BackgroundSprites } from './sprites';
import type { WorldState, OctopusEntity } from './world';
import { worldToScreen } from './camera';

// --- Ambient effects (seeded per pool, deterministic from seed) ---

interface AmbientState {
  lightRays: { x: number; width: number; angle: number; speed: number; alpha: number }[];
  plankton: { x: number; y: number; dx: number; dy: number; size: number; alpha: number }[];
}

function seededRandom(seed: number): () => number {
  let s = seed;
  return () => {
    s = (s * 16807 + 0) % 2147483647;
    return (s - 1) / 2147483646;
  };
}

const ambientCache = new Map<number, AmbientState>();

function getAmbient(seed: number, width: number, height: number): AmbientState {
  const cached = ambientCache.get(seed);
  if (cached) return cached;

  const rng = seededRandom(seed);

  const rayCount = 3 + Math.floor(rng() * 3);
  const lightRays = [];
  for (let i = 0; i < rayCount; i++) {
    lightRays.push({
      x: rng() * width,
      width: 30 + rng() * 60,
      angle: -0.15 + rng() * 0.3,
      speed: 0.003 + rng() * 0.005,
      alpha: 0.08 + rng() * 0.07,
    });
  }

  const planktonCount = 20 + Math.floor(rng() * 16);
  const plankton = [];
  for (let i = 0; i < planktonCount; i++) {
    plankton.push({
      x: rng() * width,
      y: rng() * height,
      dx: (rng() - 0.5) * 0.3,
      dy: -0.05 - rng() * 0.15,
      size: 1.5 + rng() * 1.5,
      alpha: 0.2 + rng() * 0.2,
    });
  }

  const state = { lightRays, plankton };
  ambientCache.set(seed, state);
  return state;
}

function drawLightRays(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  seed: number,
): void {
  const ambient = getAmbient(seed, width, height);

  for (const ray of ambient.lightRays) {
    const sway = Math.sin(time * ray.speed) * 30;
    const x = ray.x + sway;
    const pulse = 0.7 + 0.3 * Math.sin(time * ray.speed * 0.7 + ray.x);

    ctx.save();
    ctx.globalAlpha = ray.alpha * pulse;
    ctx.translate(x, 0);
    ctx.rotate(ray.angle);

    const grad = ctx.createLinearGradient(0, 0, 0, height * 0.8);
    grad.addColorStop(0, 'rgba(200, 230, 255, 0.8)');
    grad.addColorStop(0.4, 'rgba(160, 210, 245, 0.3)');
    grad.addColorStop(1, 'rgba(120, 190, 235, 0)');

    ctx.fillStyle = grad;
    ctx.beginPath();
    ctx.moveTo(-ray.width / 2, 0);
    ctx.lineTo(ray.width / 2, 0);
    ctx.lineTo(ray.width * 0.8, height * 0.8);
    ctx.lineTo(-ray.width * 0.8, height * 0.8);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }
}

function drawPlankton(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  time: number,
  seed: number,
): void {
  const ambient = getAmbient(seed, width, height);
  const t = time * 0.001;

  for (const p of ambient.plankton) {
    const x = (((p.x + p.dx * t * 60 + Math.sin(t * 0.5 + p.y) * 8) % width) + width) % width;
    const y = (((p.y + p.dy * t * 60 + Math.cos(t * 0.3 + p.x) * 4) % height) + height) % height;
    const pulse = 0.6 + 0.4 * Math.sin(t * 2 + p.x * 0.1);

    ctx.globalAlpha = p.alpha * pulse;
    ctx.fillStyle = 'rgba(180, 210, 240, 0.8)';
    ctx.fillRect(Math.round(x), Math.round(y), Math.round(p.size), Math.round(p.size));
  }
  ctx.globalAlpha = 1;
}

/** Draw a pixel-art style bubble. */
function drawPixelBubble(
  ctx: CanvasRenderingContext2D,
  sx: number,
  sy: number,
  sr: number,
  alpha: number,
): void {
  ctx.globalAlpha = alpha;
  ctx.strokeStyle = '#8ecae6';
  ctx.lineWidth = Math.max(1, sr * 0.3);
  ctx.beginPath();
  ctx.arc(sx, sy, sr, 0, Math.PI * 2);
  ctx.stroke();
  ctx.fillStyle = 'rgba(142, 202, 230, 0.15)';
  ctx.fill();
  ctx.fillStyle = '#c7ecff';
  const hlSize = Math.max(1, sr * 0.35);
  ctx.fillRect(sx - sr * 0.35, sy - sr * 0.45, hlSize, hlSize);
  ctx.globalAlpha = 1;
}

/** Draw seabed decorations (seaweed, shells, starfish) anchored to the canvas bottom. */
function drawDecorations(ctx: CanvasRenderingContext2D, world: WorldState, sprites: Sprites): void {
  const { camera } = world;

  for (const d of world.decorations) {
    const sprite = sprites.decor[d.type];
    if (!sprite) continue;

    // Use world X for horizontal position, but anchor to canvas bottom (sand line)
    const [sx] = worldToScreen(camera, d.x, d.y);
    if (sx < -50 || sx > camera.width + 50) continue;

    const scale = camera.zoom * 1.2;
    const drawW = sprite.width * scale;
    const drawH = sprite.height * scale;

    // Place with bottom edge at canvas bottom, slightly buried in sand
    const bottomY = camera.height + drawH * 0.15;

    ctx.save();
    ctx.globalAlpha = 0.7;
    ctx.drawImage(sprite, sx - drawW / 2, bottomY - drawH, drawW, drawH);
    ctx.restore();
  }
}

/** Draw fauna creatures behind octopuses. */
function drawFauna(
  ctx: CanvasRenderingContext2D,
  world: WorldState,
  sprites: Sprites,
  time: number,
): void {
  const { camera } = world;

  for (const f of world.fauna) {
    const sheet = sprites.fauna[f.type];
    if (!sheet) continue;

    const [sx, sy] = worldToScreen(camera, f.x, f.y);
    if (sx < -200 || sx > camera.width + 200) continue;

    const size = f.size * camera.zoom;
    const bob = Math.sin(time * 0.001 + f.x * 0.2) * 3 * camera.zoom;

    // Square frames from sprite sheet
    const FRAME_H = sheet.height;
    const FRAME_W = FRAME_H;
    const frameCount = Math.max(1, Math.floor(sheet.width / FRAME_W));
    const frame = f.animFrame % frameCount;

    ctx.save();
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = 'high';
    ctx.globalAlpha = f.alpha;
    ctx.translate(sx, sy + bob);
    if (f.vx < 0) ctx.scale(-1, 1);

    ctx.drawImage(sheet, frame * FRAME_W, 0, FRAME_W, FRAME_H, -size / 2, -size / 2, size, size);
    ctx.imageSmoothingEnabled = false;
    ctx.restore();
  }
}

/** Map node colors to hue-rotate degrees to tint the landmark sprite. */
function colorToHue(color: string): number {
  const hues: Record<string, number> = {
    '#f472b6': 0, // coral pink — base hue of the sprite is already bluish, shift to pink
    '#2dd4bf': 120, // teal
    '#fbbf24': 40, // amber
    '#a78bfa': 220, // lavender
    '#34d399': 100, // emerald
    '#60a5fa': 180, // sky blue
    '#fb923c': 20, // tangerine
    '#e879f9': 260, // fuchsia
    '#4ade80': 90, // lime
    '#38bdf8': 160, // cyan
  };
  return hues[color] ?? 0;
}

/** Screen-space bounds of the node landmark (for click hit testing). */
export interface LandmarkBounds {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Last rendered landmark bounds per node index. */
let lastLandmarkBounds: LandmarkBounds | null = null;

/** Check if a screen-space click hits the node landmark. */
export function hitTestLandmark(screenX: number, screenY: number): boolean {
  if (!lastLandmarkBounds) return false;
  const b = lastLandmarkBounds;
  return screenX >= b.x && screenX <= b.x + b.w && screenY >= b.y && screenY <= b.y + b.h;
}

/** Draw node landmark in the bottom-left corner of the canvas. */
function drawNodeLandmarks(
  ctx: CanvasRenderingContext2D,
  world: WorldState,
  sprites: Sprites,
): void {
  const { camera } = world;
  const sprite = sprites.nodes['sunken-ship'] ?? sprites.nodes['shipwreck'];
  if (!sprite || world.nodes.length === 0) {
    lastLandmarkBounds = null;
    return;
  }

  const node = world.nodes[0];

  // Landmark in the bottom-left corner, sized relative to canvas
  const drawH = Math.round(camera.height * 0.45);
  const drawW = Math.round((sprite.width / sprite.height) * drawH);
  const margin = 12;
  const x = margin;
  const y = camera.height - drawH - 2;

  // Store bounds for hit testing
  lastLandmarkBounds = { x, y, w: drawW, h: drawH };

  // Enable smoothing for the landmark so it doesn't look blocky when scaled up
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';

  // Tint per node color
  const hue = colorToHue(node.color);
  if (hue !== 0) ctx.filter = `hue-rotate(${hue}deg)`;
  ctx.globalAlpha = 0.85;
  ctx.drawImage(sprite, x, y, drawW, drawH);
  ctx.filter = 'none';
  ctx.globalAlpha = 1;

  // Restore pixel-art rendering for everything else
  ctx.imageSmoothingEnabled = false;

  // Node name label centered on the ship
  const fontSize = Math.max(10, Math.round(drawH * 0.09));
  ctx.font = `bold ${fontSize}px monospace`;
  ctx.textAlign = 'center';
  const label = node.name;
  const textW = ctx.measureText(label).width;
  const padX = 6;
  const padY = 3;
  const labelW = textW + padX * 2;
  const labelH = fontSize + padY * 2;
  const labelX = Math.round(x + drawW / 2 - labelW / 2);
  const labelY = Math.round(y + drawH - labelH - 4);

  ctx.fillStyle = 'rgba(8, 18, 30, 0.7)';
  ctx.fillRect(labelX, labelY, labelW, labelH);
  ctx.fillStyle = node.color;
  ctx.fillText(label, x + drawW / 2, labelY + padY + fontSize - 1);
}

export function render(
  ctx: CanvasRenderingContext2D,
  world: WorldState,
  sprites: Sprites,
  time: number,
  background?: BackgroundSprites,
  hueRotate = 0,
  ambientSeed = 0,
): void {
  const { camera } = world;
  const { width, height } = camera;

  ctx.imageSmoothingEnabled = false;

  const bg = background;
  const hasBg = bg && Object.keys(bg).length > 0;

  if (hasBg) {
    // --- Single full-canvas background ---
    const bgSprite = bg['sea-background'] ?? bg['water-surface'];
    if (bgSprite) {
      if (hueRotate !== 0) ctx.filter = `hue-rotate(${hueRotate}deg)`;
      ctx.drawImage(bgSprite, 0, 0, bgSprite.width, bgSprite.height, 0, 0, width, height);
      ctx.filter = 'none';
    }
  } else {
    const grad = ctx.createLinearGradient(0, 0, 0, height);
    grad.addColorStop(0, '#1478a7');
    grad.addColorStop(1, '#0a4f7a');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, width, height);
  }

  // Ensure no CSS filter bleeds into subsequent draws
  ctx.filter = 'none';

  // --- Ambient effects ---
  drawLightRays(ctx, width, height, time, ambientSeed);
  drawPlankton(ctx, width, height, time, ambientSeed);

  // --- Seabed decorations ---
  drawDecorations(ctx, world, sprites);

  // --- Fauna ---
  drawFauna(ctx, world, sprites, time);

  // --- Node landmarks ---
  drawNodeLandmarks(ctx, world, sprites);

  // --- Octopuses ---
  const nodeByName = new Map(world.nodes.map((n) => [n.name, n]));
  for (const oct of world.octopuses) {
    drawOctopus(ctx, oct, sprites, world, time, nodeByName);
  }

  // --- Bubbles ---
  for (const bubble of world.bubbles) {
    const [sx, sy] = worldToScreen(camera, bubble.x, bubble.y);
    const sr = bubble.radius * camera.zoom;
    drawPixelBubble(ctx, sx, sy, sr, bubble.alpha);
  }

  // --- Empty state ---
  if (world.octopuses.length === 0) {
    const msg = 'No active sessions \u2014 the ocean is calm';
    ctx.font = '14px monospace';
    ctx.textAlign = 'left';
    const textW = ctx.measureText(msg).width;
    const padX = 12;
    const padY = 8;
    const boxW = textW + padX * 2;
    const boxH = 14 + padY * 2;
    const boxX = Math.round(width / 2 - boxW / 2);
    const boxY = Math.round(height / 2 - boxH / 2);
    ctx.fillStyle = 'rgba(8, 18, 30, 0.65)';
    ctx.fillRect(boxX, boxY, boxW, boxH);
    ctx.fillStyle = '#c8dce8';
    ctx.fillText(msg, boxX + padX, boxY + padY + 12);
  }
}

function drawOctopus(
  ctx: CanvasRenderingContext2D,
  oct: OctopusEntity,
  sprites: Sprites,
  world: WorldState,
  time: number,
  nodeByName: Map<string, { color: string }>,
): void {
  const { camera } = world;
  const anim = oct.isSwimming ? 'swim' : 'idle';
  const spriteKey = `${oct.status}-${anim}`;
  const sheet = sprites.octopus[spriteKey] ?? sprites.octopus['running-idle'];
  if (!sheet) return;

  // Square frames: height = frame size, width = N * height
  const FRAME_H = sheet.height;
  const FRAME_W = FRAME_H;
  const frameCount = Math.max(1, Math.floor(sheet.width / FRAME_W));
  const currentFrame = oct.animFrame % frameCount;

  const [sx, sy] = worldToScreen(camera, oct.x, oct.y);
  const size = 160 * (camera.zoom / 2);
  const drawW = size;
  const drawH = size;

  const bobSpeed = oct.status === 'dead' ? 0.5 : oct.status === 'stale' ? 1.0 : 1.5;
  const bobAmount = oct.status === 'dead' ? 1 : 2;
  const bob = Math.sin(time * 0.002 * bobSpeed + oct.x * 0.3) * bobAmount * camera.zoom;

  const breathe = Math.sin(time * 0.003 + oct.y * 0.2) * 0.03;
  const scaleX = 1.0 - breathe;
  const scaleY = 1.0 + breathe;

  const drawY = sy + bob;
  const flipX = oct.vx < -0.1;

  ctx.save();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  ctx.translate(sx, drawY);
  ctx.scale(flipX ? -scaleX : scaleX, scaleY);

  ctx.drawImage(
    sheet,
    currentFrame * FRAME_W,
    0,
    FRAME_W,
    FRAME_H,
    -drawW / 2,
    -drawH / 2,
    drawW,
    drawH,
  );
  ctx.imageSmoothingEnabled = false;
  ctx.restore();

  // Name + provider badges
  const nodeColor = nodeByName.get(oct.nodeName)?.color ?? '#d4e4ef';
  const fontSize = Math.max(10, 14 * (camera.zoom / 2));
  const padX = 4;
  const padY = 2;
  const gap = 2;
  ctx.textAlign = 'left';
  let badgeY = drawY + drawH * 0.28 + 4;

  ctx.font = `${fontSize}px monospace`;
  const nameTextW = ctx.measureText(oct.name).width;
  const nameW = nameTextW + padX * 2;
  const nameH = fontSize + padY * 2;
  const nameX = Math.round(sx - nameW / 2);
  const nameY = Math.round(badgeY);
  ctx.fillStyle = 'rgba(8, 18, 30, 0.7)';
  ctx.fillRect(nameX, nameY, nameW, nameH);
  ctx.fillStyle = nodeColor;
  ctx.fillText(oct.name, nameX + padX, nameY + padY + fontSize - 1);
  badgeY += nameH + gap;

  if (oct.provider) {
    ctx.font = `${fontSize - 1}px monospace`;
    const provTextW = ctx.measureText(oct.provider).width;
    const provW = provTextW + padX * 2;
    const provH = fontSize - 1 + padY * 2;
    const provX = Math.round(sx - provW / 2);
    const provY = Math.round(badgeY);
    ctx.fillStyle = 'rgba(8, 18, 30, 0.5)';
    ctx.fillRect(provX, provY, provW, provH);
    ctx.fillStyle = '#94b8d0';
    ctx.fillText(oct.provider, provX + padX, provY + padY + fontSize - 2);
  }

  if (oct.waitingForInput) {
    const bubble = sprites.ui['speech-bubble'];
    if (bubble) {
      const bSize = 16 * camera.zoom;
      const bounce = Math.sin(time * 0.004) * 3;
      ctx.drawImage(bubble, sx + size / 4, drawY - size / 2 - bSize + bounce, bSize, bSize);
    }
  }
}
