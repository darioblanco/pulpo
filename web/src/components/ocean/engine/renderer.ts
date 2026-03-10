import type { Sprites, BackgroundSprites } from './sprites';
import type { WorldState, OctopusEntity } from './world';
import { worldToScreen } from './camera';

// Parallax scroll factors (0 = static, 1 = moves with camera)
const PARALLAX: Record<string, number> = {
  'water-surface': 0.1,
  fish: 0.3,
  rocks: 0.8,
  sand: 0.9,
  'coral-foreground': 1.0,
};

// --- Ambient effects (seeded per pool, deterministic from seed) ---

interface AmbientState {
  lightRays: { x: number; width: number; angle: number; speed: number; alpha: number }[];
  plankton: { x: number; y: number; dx: number; dy: number; size: number; alpha: number }[];
}

/** Seeded pseudo-random for deterministic ambient placement. */
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

  // 3-5 light rays from above
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

  // 20-35 drifting plankton particles
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

/** Draw subtle light rays from the surface. */
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

/** Draw tiny drifting plankton/dust particles. */
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
    // Slow looping drift
    const x = (((p.x + p.dx * t * 60 + Math.sin(t * 0.5 + p.y) * 8) % width) + width) % width;
    const y = (((p.y + p.dy * t * 60 + Math.cos(t * 0.3 + p.x) * 4) % height) + height) % height;
    const pulse = 0.6 + 0.4 * Math.sin(t * 2 + p.x * 0.1);

    ctx.globalAlpha = p.alpha * pulse;
    ctx.fillStyle = 'rgba(180, 210, 240, 0.8)';
    ctx.fillRect(Math.round(x), Math.round(y), Math.round(p.size), Math.round(p.size));
  }
  ctx.globalAlpha = 1;
}

/** Draw a parallax background layer, tiled horizontally. */
function drawParallaxLayer(
  ctx: CanvasRenderingContext2D,
  sprite: HTMLImageElement | undefined,
  camera: { x: number; width: number; height: number; zoom: number },
  parallaxFactor: number,
  time: number,
): void {
  if (!sprite) return;

  const scale = camera.height / sprite.height;
  const tileW = Math.ceil(sprite.width * scale);
  const tileH = camera.height;

  const drift = time * 0.008 * (1 - parallaxFactor);
  const cameraOffset = camera.x * camera.zoom * parallaxFactor;
  const rawOffset = -(cameraOffset + drift);
  const offset = ((rawOffset % tileW) + tileW) % tileW;

  for (let x = offset - tileW; x < camera.width; x += tileW) {
    ctx.drawImage(sprite, 0, 0, sprite.width, sprite.height, x, 0, tileW, tileH);
  }
}

/**
 * Draw a parallax layer clipped to the region below `clipY`.
 */
function drawGroundLayer(
  ctx: CanvasRenderingContext2D,
  sprite: HTMLImageElement | undefined,
  camera: { x: number; width: number; height: number; zoom: number },
  parallaxFactor: number,
  time: number,
  clipY: number,
): void {
  if (!sprite) return;

  ctx.save();
  ctx.beginPath();
  ctx.rect(0, clipY, camera.width, camera.height - clipY);
  ctx.clip();

  drawParallaxLayer(ctx, sprite, camera, parallaxFactor, time);

  ctx.restore();
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
    // Apply hue rotation for color variation
    if (hueRotate !== 0) {
      ctx.filter = `hue-rotate(${hueRotate}deg)`;
    }

    // Atmosphere layers (full viewport)
    drawParallaxLayer(ctx, bg['water-surface'], camera, PARALLAX['water-surface'], time);
    drawParallaxLayer(ctx, bg['fish'], camera, PARALLAX['fish'], time);

    // Ground layers clipped to seabed region
    const [, seabedScreenY] = worldToScreen(camera, 0, 190);
    drawGroundLayer(ctx, bg['rocks'], camera, PARALLAX['rocks'], time, seabedScreenY);
    drawGroundLayer(ctx, bg['sand'], camera, PARALLAX['sand'], time, seabedScreenY);
    drawGroundLayer(
      ctx,
      bg['coral-foreground'],
      camera,
      PARALLAX['coral-foreground'],
      time,
      seabedScreenY,
    );

    // Reset filter before drawing octopuses/text
    ctx.filter = 'none';
  } else {
    const grad = ctx.createLinearGradient(0, 0, 0, height);
    grad.addColorStop(0, '#1478a7');
    grad.addColorStop(1, '#0a4f7a');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, width, height);
  }

  // --- Ambient effects (behind octopuses) ---
  drawLightRays(ctx, width, height, time, ambientSeed);
  drawPlankton(ctx, width, height, time, ambientSeed);

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

  const FRAME_W = 32;
  const FRAME_H = 32;
  const frameCount = Math.max(1, Math.floor(sheet.width / FRAME_W));
  const currentFrame = oct.animFrame % frameCount;
  const nextFrame = (oct.animFrame + 1) % frameCount;

  // Cross-fade progress
  const fps = oct.isSwimming ? 5 : 3;
  const frameDuration = 1 / fps;
  const crossfade = Math.min(oct.animTimer / frameDuration, 1);

  const [sx, sy] = worldToScreen(camera, oct.x, oct.y);
  const size = FRAME_W * camera.zoom;

  // Gentle vertical bob
  const bobSpeed = oct.status === 'dead' ? 0.5 : oct.status === 'stale' ? 1.0 : 1.5;
  const bobAmount = oct.status === 'dead' ? 1 : 2;
  const bob = Math.sin(time * 0.002 * bobSpeed + oct.x * 0.3) * bobAmount * camera.zoom;

  // Subtle breathing squash/stretch
  const breathe = Math.sin(time * 0.003 + oct.y * 0.2) * 0.03;
  const scaleX = 1.0 - breathe;
  const scaleY = 1.0 + breathe;

  const drawY = sy + bob;
  const flipX = oct.vx < -0.1;

  ctx.save();
  ctx.translate(sx, drawY);
  ctx.scale(flipX ? -scaleX : scaleX, scaleY);

  ctx.drawImage(
    sheet,
    currentFrame * FRAME_W,
    0,
    FRAME_W,
    FRAME_H,
    -size / 2,
    -size / 2,
    size,
    size,
  );
  if (crossfade > 0.3 && currentFrame !== nextFrame) {
    const blendAlpha = (crossfade - 0.3) / 0.7;
    ctx.globalAlpha = blendAlpha;
    ctx.drawImage(
      sheet,
      nextFrame * FRAME_W,
      0,
      FRAME_W,
      FRAME_H,
      -size / 2,
      -size / 2,
      size,
      size,
    );
    ctx.globalAlpha = 1;
  }
  ctx.restore();

  // Name + provider badges — centered below octopus
  const nodeColor = nodeByName.get(oct.nodeName)?.color ?? '#d4e4ef';
  const fontSize = Math.max(8, 9 * (camera.zoom / 2));
  const padX = 4;
  const padY = 2;
  const gap = 2;
  ctx.textAlign = 'left';
  let badgeY = drawY + size / 2 + 4;

  // Name badge
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

  // Provider badge
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

  // Speech bubble for waiting-for-input
  if (oct.waitingForInput) {
    const bubble = sprites.ui['speech-bubble'];
    if (bubble) {
      const bSize = 16 * camera.zoom;
      const bounce = Math.sin(time * 0.004) * 3;
      ctx.drawImage(bubble, sx + size / 4, drawY - size / 2 - bSize + bounce, bSize, bSize);
    }
  }
}
