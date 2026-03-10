import type { Sprites } from './sprites';
import type { WorldState, OctopusEntity } from './world';
import { worldToScreen } from './camera';

const BG_TOP = '#0c1929';
const BG_BOTTOM = '#050e1a';
const SEABED_SURFACE = '#0f1d2d';
const SEABED_DEEP = '#0a1420';

export function render(
  ctx: CanvasRenderingContext2D,
  world: WorldState,
  sprites: Sprites,
  time: number,
): void {
  const { camera } = world;
  const { width, height } = camera;

  ctx.imageSmoothingEnabled = false;

  // --- Background gradient ---
  const grad = ctx.createLinearGradient(0, 0, 0, height);
  grad.addColorStop(0, BG_TOP);
  grad.addColorStop(1, BG_BOTTOM);
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, width, height);

  // --- Caustic light blobs ---
  ctx.globalAlpha = 0.03;
  ctx.fillStyle = '#4488cc';
  for (let i = 0; i < 10; i++) {
    const cx = (Math.sin(time * 0.0003 + i * 1.7) * 0.5 + 0.5) * width;
    const cy = (Math.cos(time * 0.0002 + i * 2.3) * 0.3 + 0.35) * height;
    const cr = 50 + Math.sin(time * 0.001 + i) * 20;
    ctx.beginPath();
    ctx.arc(cx, cy, cr, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.globalAlpha = 1;

  // --- Seabed ---
  const [, seabedScreenY] = worldToScreen(camera, 0, 220);
  if (seabedScreenY < height) {
    const seabedGrad = ctx.createLinearGradient(0, seabedScreenY, 0, height);
    seabedGrad.addColorStop(0, SEABED_SURFACE);
    seabedGrad.addColorStop(1, SEABED_DEEP);
    ctx.fillStyle = seabedGrad;
    ctx.fillRect(0, seabedScreenY, width, height - seabedScreenY);
  }

  // --- Water surface ---
  const [, surfaceY] = worldToScreen(camera, 0, 0);
  if (surfaceY > 0 && surfaceY < height) {
    ctx.strokeStyle = '#1e3a5f';
    ctx.globalAlpha = 0.3;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, surfaceY);
    for (let x = 0; x <= width; x += 20) {
      ctx.lineTo(x, surfaceY + Math.sin(x * 0.02 + time * 0.001) * 3);
    }
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  // --- Decorations ---
  for (const deco of world.decorations) {
    const sprite = sprites.decor[deco.type];
    if (!sprite) continue;
    const [sx, sy] = worldToScreen(camera, deco.x, deco.y);
    const sw = sprite.width * camera.zoom * 0.5;
    const sh = sprite.height * camera.zoom * 0.5;
    ctx.drawImage(sprite, sx - sw / 2, sy - sh, sw, sh);
  }

  // --- Node landmarks ---
  for (const node of world.nodes) {
    const spriteKey = node.isLocal
      ? 'coral-reef'
      : node.status === 'online'
        ? 'sunken-ship'
        : 'shipwreck';
    const sprite = sprites.nodes[spriteKey];
    if (!sprite) continue;

    const [sx, sy] = worldToScreen(camera, node.x, node.y);
    const sw = sprite.width * camera.zoom * 0.8;
    const sh = sprite.height * camera.zoom * 0.8;
    ctx.drawImage(sprite, sx - sw / 2, sy - sh, sw, sh);

    // Node name
    const fontSize = Math.max(10, 12 * (camera.zoom / 2));
    ctx.font = `bold ${fontSize}px monospace`;
    ctx.textAlign = 'center';
    ctx.fillStyle = node.status === 'online' ? '#7dd3fc' : '#64748b';
    ctx.fillText(node.name, sx, sy + fontSize + 4);

    // Status dot
    const dotR = Math.max(2, 3 * (camera.zoom / 2));
    ctx.beginPath();
    ctx.arc(sx + sw / 2 - dotR * 2, sy - sh + dotR * 2, dotR, 0, Math.PI * 2);
    ctx.fillStyle =
      node.status === 'online' ? '#34d399' : node.status === 'offline' ? '#f87171' : '#94a3b8';
    ctx.fill();
  }

  // --- Octopuses ---
  for (const oct of world.octopuses) {
    drawOctopus(ctx, oct, sprites, world, time);
  }

  // --- Bubbles ---
  for (const bubble of world.bubbles) {
    const [sx, sy] = worldToScreen(camera, bubble.x, bubble.y);
    const sr = bubble.radius * camera.zoom;
    ctx.beginPath();
    ctx.arc(sx, sy, sr, 0, Math.PI * 2);
    ctx.fillStyle = `rgba(120, 200, 255, ${bubble.alpha})`;
    ctx.fill();
  }

  // --- Empty state ---
  if (world.octopuses.length === 0) {
    ctx.font = '14px monospace';
    ctx.textAlign = 'center';
    ctx.fillStyle = '#64748b';
    ctx.fillText('No active sessions \u2014 the ocean is calm', width / 2, height / 2);
  }
}

function drawOctopus(
  ctx: CanvasRenderingContext2D,
  oct: OctopusEntity,
  sprites: Sprites,
  world: WorldState,
  time: number,
): void {
  const { camera } = world;
  const anim = oct.isSwimming ? 'swim' : 'idle';
  const spriteKey = `${oct.status}-${anim}`;
  const sheet = sprites.octopus[spriteKey] ?? sprites.octopus['running-idle'];
  if (!sheet) return;

  const FRAME_W = 32;
  const FRAME_H = 32;
  const frameCount = Math.max(1, Math.floor(sheet.width / FRAME_W));
  const frame = oct.animFrame % frameCount;

  const [sx, sy] = worldToScreen(camera, oct.x, oct.y);
  const size = FRAME_W * camera.zoom;

  // Flip sprite when moving left
  ctx.save();
  if (oct.vx < -0.1) {
    ctx.translate(sx, sy);
    ctx.scale(-1, 1);
    ctx.drawImage(sheet, frame * FRAME_W, 0, FRAME_W, FRAME_H, -size / 2, -size / 2, size, size);
  } else {
    ctx.drawImage(
      sheet,
      frame * FRAME_W,
      0,
      FRAME_W,
      FRAME_H,
      sx - size / 2,
      sy - size / 2,
      size,
      size,
    );
  }
  ctx.restore();

  // Name label
  const fontSize = Math.max(8, 9 * (camera.zoom / 2));
  ctx.font = `${fontSize}px monospace`;
  ctx.textAlign = 'center';
  ctx.fillStyle = '#94a3b8';
  const label = oct.name.length > 12 ? `${oct.name.slice(0, 11)}\u2026` : oct.name;
  ctx.fillText(label, sx, sy + size / 2 + fontSize + 2);

  // Provider badge
  if (oct.provider) {
    ctx.font = `${fontSize - 1}px monospace`;
    ctx.fillStyle = '#64748b';
    ctx.fillText(oct.provider, sx, sy + size / 2 + fontSize * 2 + 4);
  }

  // Speech bubble for waiting-for-input
  if (oct.waitingForInput) {
    const bubble = sprites.ui['speech-bubble'];
    if (bubble) {
      const bSize = 16 * camera.zoom;
      const bounce = Math.sin(time * 0.004) * 3;
      ctx.drawImage(bubble, sx + size / 4, sy - size / 2 - bSize + bounce, bSize, bSize);
    }
  }
}
