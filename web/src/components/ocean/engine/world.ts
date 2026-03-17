import type { Session, NodeInfo, PeerInfo } from '@/api/types';
import type { Camera } from './camera';
import { createCamera, fitCamera } from './camera';

// --- Layout constants ---
const NODE_SPACING = 250;
const SWIM_ZONE_TOP = 50;
const SWIM_ZONE_BOTTOM = 190;
const SEABED_Y = 230;
const NODE_Y = SEABED_Y;

// --- Node color palette ---
export const NODE_COLORS = [
  '#f472b6', // coral pink
  '#2dd4bf', // ocean teal
  '#fbbf24', // amber gold
  '#a78bfa', // lavender
  '#34d399', // emerald
  '#60a5fa', // sky blue
  '#fb923c', // tangerine
  '#e879f9', // fuchsia
  '#4ade80', // lime
  '#38bdf8', // cyan
];

// --- Entity types ---

export interface OctopusEntity {
  sessionId: string;
  name: string;
  status: string;
  command: string;
  description: string | null;
  ink: string | null;
  workdir: string;
  createdAt: string;
  lastOutputAt: string | null;
  interventionReason: string | null;
  nodeName: string;
  x: number;
  y: number;
  homeX: number;
  homeY: number;
  vx: number;
  vy: number;
  animFrame: number;
  animTimer: number;
  isSwimming: boolean;
  wanderTimer: number;
  wanderTargetX: number;
  wanderTargetY: number;
}

export interface NodeLandmark {
  name: string;
  isLocal: boolean;
  status: 'online' | 'offline' | 'unknown';
  x: number;
  y: number;
  color: string;
  sessionCount: number;
}

export interface Decoration {
  type: string;
  x: number;
  y: number;
}

export interface Bubble {
  x: number;
  y: number;
  radius: number;
  speed: number;
  alpha: number;
}

export interface FaunaEntity {
  type: string;
  x: number;
  y: number;
  vx: number;
  size: number;
  alpha: number;
  animFrame: number;
  animTimer: number;
  animSpeed: number;
}

export interface WorldState {
  camera: Camera;
  nodes: NodeLandmark[];
  octopuses: OctopusEntity[];
  decorations: Decoration[];
  bubbles: Bubble[];
  fauna: FaunaEntity[];
}

// --- Behavior config per status ---

interface BehaviorConfig {
  radius: number;
  speed: number;
  intervalMin: number;
  intervalMax: number;
}

const BEHAVIOR: Record<string, BehaviorConfig> = {
  active: { radius: 60, speed: 30, intervalMin: 1.5, intervalMax: 3 },
  creating: { radius: 30, speed: 15, intervalMin: 2, intervalMax: 4 },
  idle: { radius: 8, speed: 5, intervalMin: 4, intervalMax: 8 },
  lost: { radius: 8, speed: 5, intervalMin: 4, intervalMax: 8 },
  ready: { radius: 40, speed: 10, intervalMin: 3, intervalMax: 6 },
  killed: { radius: 5, speed: 2, intervalMin: 5, intervalMax: 10 },
};

function randomBetween(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

// --- Create world ---

// Fish types swim in schools; larger creatures are rarer and slower
const FISH_TYPES = ['angelfish', 'clownfish', 'fish-gold', 'silverfish', 'tang'];
const SHARK_TYPES = ['shark-2', 'shark-3', 'shark-5', 'shark-6'];
const LARGE_TYPES = ['jellyfish', 'turtle'];

function generateFauna(width: number): FaunaEntity[] {
  const fauna: FaunaEntity[] = [];
  const worldW = Math.max(width, 600);

  // 2-3 schools of fish, each school is 3-5 fish of the same type clustered together
  const schoolCount = 2 + Math.floor(Math.random() * 2);
  for (let s = 0; s < schoolCount; s++) {
    const type = FISH_TYPES[Math.floor(Math.random() * FISH_TYPES.length)];
    const schoolSize = 3 + Math.floor(Math.random() * 3);
    const centerX = randomBetween(-worldW * 0.3, worldW * 1.3);
    const centerY = randomBetween(SWIM_ZONE_TOP + 30, SWIM_ZONE_BOTTOM - 30);
    const dir = Math.random() > 0.5 ? 1 : -1;
    const baseSpeed = randomBetween(12, 22);

    for (let i = 0; i < schoolSize; i++) {
      fauna.push({
        type,
        x: centerX + randomBetween(-40, 40),
        y: centerY + randomBetween(-25, 25),
        vx: (baseSpeed + randomBetween(-3, 3)) * dir,
        size: randomBetween(28, 42),
        alpha: 1,
        animFrame: Math.floor(Math.random() * 4),
        animTimer: 0,
        animSpeed: randomBetween(0.15, 0.3),
      });
    }
  }

  // 2-4 solo fish scattered around
  const soloCount = 2 + Math.floor(Math.random() * 3);
  for (let i = 0; i < soloCount; i++) {
    const type = FISH_TYPES[Math.floor(Math.random() * FISH_TYPES.length)];
    fauna.push({
      type,
      x: randomBetween(-worldW * 0.5, worldW * 1.5),
      y: randomBetween(SWIM_ZONE_TOP + 20, SWIM_ZONE_BOTTOM - 10),
      vx: randomBetween(10, 25) * (Math.random() > 0.5 ? 1 : -1),
      size: randomBetween(30, 45),
      alpha: 1,
      animFrame: Math.floor(Math.random() * 4),
      animTimer: 0,
      animSpeed: randomBetween(0.15, 0.3),
    });
  }

  // 1-2 large creatures (jellyfish, turtle)
  const largeCount = 1 + Math.floor(Math.random() * 2);
  for (let i = 0; i < largeCount; i++) {
    const type = LARGE_TYPES[Math.floor(Math.random() * LARGE_TYPES.length)];
    fauna.push({
      type,
      x: randomBetween(-worldW * 0.3, worldW * 1.3),
      y: randomBetween(SWIM_ZONE_TOP + 40, SWIM_ZONE_BOTTOM - 20),
      vx: randomBetween(3, 8) * (Math.random() > 0.5 ? 1 : -1),
      size: randomBetween(50, 70),
      alpha: 1,
      animFrame: Math.floor(Math.random() * 4),
      animTimer: 0,
      animSpeed: randomBetween(0.1, 0.2),
    });
  }

  // 1-3 bubble columns drifting up
  const bubbleCount = 1 + Math.floor(Math.random() * 3);
  for (let i = 0; i < bubbleCount; i++) {
    fauna.push({
      type: 'bubbles',
      x: randomBetween(-worldW * 0.3, worldW * 1.3),
      y: randomBetween(SWIM_ZONE_TOP, SWIM_ZONE_BOTTOM),
      vx: randomBetween(-1, 1),
      size: randomBetween(20, 35),
      alpha: 0.5,
      animFrame: Math.floor(Math.random() * 4),
      animTimer: 0,
      animSpeed: randomBetween(0.3, 0.5),
    });
  }

  // 0-1 shark (rare, large, slow)
  if (Math.random() < 0.5) {
    const type = SHARK_TYPES[Math.floor(Math.random() * SHARK_TYPES.length)];
    fauna.push({
      type,
      x: randomBetween(-worldW * 0.5, worldW * 1.5),
      y: randomBetween(SWIM_ZONE_TOP + 10, SWIM_ZONE_TOP + 60),
      vx: randomBetween(6, 12) * (Math.random() > 0.5 ? 1 : -1),
      size: randomBetween(60, 90),
      alpha: 1,
      animFrame: 0,
      animTimer: 0,
      animSpeed: 1, // no animation for sharks (single frame repeated)
    });
  }

  return fauna;
}

export function createWorld(width: number, height: number): WorldState {
  return {
    camera: createCamera(width, height),
    nodes: [],
    octopuses: [],
    decorations: [],
    bubbles: [],
    fauna: generateFauna(width),
  };
}

// --- Generate seabed decorations ---

function generateDecorations(nodes: NodeLandmark[]): Decoration[] {
  if (nodes.length === 0) return [];

  const minX = Math.min(...nodes.map((n) => n.x)) - 300;
  const maxX = Math.max(...nodes.map((n) => n.x)) + 300;
  const types = ['seaweed-1', 'seaweed-2', 'shell-1', 'shell-2', 'starfish'];
  const count = Math.max(15, nodes.length * 8);
  const decorations: Decoration[] = [];

  // Spread evenly across the range with jitter to avoid clumping
  const spacing = (maxX - minX) / count;
  for (let i = 0; i < count; i++) {
    decorations.push({
      type: types[Math.floor(Math.random() * types.length)],
      x: minX + i * spacing + randomBetween(-spacing * 0.4, spacing * 0.4),
      y: SEABED_Y + randomBetween(-5, 5),
    });
  }

  return decorations;
}

// --- Status-based home zones ---
//
// Each status has a distinct region so sessions are visually grouped:
//   Active/Creating — center-left, upper water (wide swim radius)
//   Idle            — right side, mid-water (barely moves)
//   Lost            — lower-right (drifting near bottom)
//   Ready         — upper-left (floats upward)
//   Killed          — bottom, near seabed (sinks)

interface StatusZone {
  xOffset: number;
  yBase: number;
  cols: number;
  spacingX: number;
  spacingY: number;
}

const STATUS_ZONES: Record<string, StatusZone> = {
  active: { xOffset: -40, yBase: SWIM_ZONE_TOP + 30, cols: 3, spacingX: 65, spacingY: 90 },
  creating: { xOffset: -40, yBase: SWIM_ZONE_TOP + 30, cols: 3, spacingX: 65, spacingY: 90 },
  idle: { xOffset: 130, yBase: SWIM_ZONE_TOP + 50, cols: 2, spacingX: 60, spacingY: 85 },
  lost: { xOffset: 80, yBase: SWIM_ZONE_BOTTOM - 30, cols: 2, spacingX: 60, spacingY: 80 },
  ready: { xOffset: -80, yBase: SWIM_ZONE_TOP + 15, cols: 2, spacingX: 60, spacingY: 85 },
  killed: { xOffset: 30, yBase: SWIM_ZONE_BOTTOM - 10, cols: 2, spacingX: 55, spacingY: 75 },
};

/** Minimum world-unit distance between octopuses before repulsion kicks in. */
export const SEPARATION_DIST = 70;

/**
 * Vertical scale factor for separation — octopuses are taller than wide
 * (sprite + labels ~85 world units tall vs ~50 wide), so we need more
 * vertical clearance. Effective vertical separation = SEPARATION_DIST * VERT_SCALE.
 */
export const SEPARATION_VERT_SCALE = 1.4;

function assignHomeForStatus(
  nodeX: number,
  status: string,
  indexInGroup: number,
): [number, number] {
  const zone = STATUS_ZONES[status] ?? STATUS_ZONES.active;
  const cols = zone.cols;
  const col = indexInGroup % cols;
  const row = Math.floor(indexInGroup / cols);
  const startX = nodeX + zone.xOffset - ((cols - 1) * zone.spacingX) / 2;
  const x = startX + col * zone.spacingX;
  const y = zone.yBase + row * zone.spacingY;
  return [x, y];
}

// --- Sync a single node into a tide pool world ---

export function syncSingleNode(
  world: WorldState,
  nodeName: string,
  isLocal: boolean,
  status: 'online' | 'offline' | 'unknown',
  sessions: Session[],
  nodeColor: string,
): void {
  const newNode: NodeLandmark = {
    name: nodeName,
    isLocal,
    status,
    x: 0,
    y: NODE_Y,
    color: nodeColor,
    sessionCount: sessions.length,
  };

  const nodesChanged = world.nodes.length !== 1 || world.nodes[0]?.name !== nodeName;

  world.nodes = [newNode];

  if (nodesChanged || world.decorations.length === 0) {
    world.decorations = generateDecorations([newNode]);
  }

  fitCamera(world.camera, [newNode]);

  // Diff octopuses — group by status for zone-based home assignment
  const existingById = new Map(world.octopuses.map((o) => [o.sessionId, o]));
  const newOctopuses: OctopusEntity[] = [];
  const statusIndex: Record<string, number> = {};

  for (let i = 0; i < sessions.length; i++) {
    const session = sessions[i];
    const idx = statusIndex[session.status] ?? 0;
    statusIndex[session.status] = idx + 1;
    const existing = existingById.get(session.id);

    if (existing) {
      const statusChanged = existing.status !== session.status;
      existing.status = session.status;
      existing.ink = session.ink;
      existing.command = session.command;
      existing.description = session.description;
      existing.workdir = session.workdir;
      existing.createdAt = session.created_at;
      existing.lastOutputAt = session.last_output_at;
      existing.interventionReason = session.intervention_reason;
      existing.nodeName = nodeName;
      if (statusChanged) {
        const [hx, hy] = assignHomeForStatus(0, session.status, idx);
        existing.homeX = hx;
        existing.homeY = hy;
      }
      newOctopuses.push(existing);
    } else {
      const [hx, hy] = assignHomeForStatus(0, session.status, idx);
      newOctopuses.push({
        sessionId: session.id,
        name: session.name,
        status: session.status,
        command: session.command,
        description: session.description,
        ink: session.ink,
        workdir: session.workdir,
        createdAt: session.created_at,
        lastOutputAt: session.last_output_at,
        interventionReason: session.intervention_reason,
        nodeName: nodeName,
        x: hx + randomBetween(-10, 10),
        y: hy + randomBetween(-10, 10),
        homeX: hx,
        homeY: hy,
        vx: 0,
        vy: 0,
        animFrame: 0,
        animTimer: 0,
        isSwimming: false,
        wanderTimer: randomBetween(1, 3),
        wanderTargetX: hx,
        wanderTargetY: hy,
      });
    }
  }

  world.octopuses = newOctopuses;
}

// --- Sync React data into world state ---

export function syncData(
  world: WorldState,
  localNode: NodeInfo,
  localSessions: Session[],
  peers: PeerInfo[],
  peerSessions: Record<string, Session[]>,
): void {
  // Map sessions to nodes (needed for session counts)
  const sessionsByNode: Record<string, Session[]> = {};
  sessionsByNode[localNode.name] = localSessions;
  for (const peer of peers) {
    sessionsByNode[peer.name] = peerSessions[peer.name] ?? [];
  }

  // Build node list
  const newNodes: NodeLandmark[] = [
    {
      name: localNode.name,
      isLocal: true,
      status: 'online',
      x: 0,
      y: NODE_Y,
      color: NODE_COLORS[0 % NODE_COLORS.length],
      sessionCount: (sessionsByNode[localNode.name] ?? []).length,
    },
  ];

  for (let i = 0; i < peers.length; i++) {
    newNodes.push({
      name: peers[i].name,
      isLocal: false,
      status: peers[i].status,
      x: (i + 1) * NODE_SPACING,
      y: NODE_Y,
      color: NODE_COLORS[(i + 1) % NODE_COLORS.length],
      sessionCount: (sessionsByNode[peers[i].name] ?? []).length,
    });
  }

  // Regenerate decorations only when node layout changes
  const nodesChanged =
    world.nodes.length !== newNodes.length ||
    world.nodes.some((n, i) => n.name !== newNodes[i]?.name);

  world.nodes = newNodes;

  if (nodesChanged || world.decorations.length === 0) {
    world.decorations = generateDecorations(newNodes);
  }

  fitCamera(world.camera, newNodes);

  // Diff octopuses: keep existing (preserve animation), add new, remove gone
  const existingById = new Map(world.octopuses.map((o) => [o.sessionId, o]));
  const newOctopuses: OctopusEntity[] = [];

  for (const node of newNodes) {
    const sessions = sessionsByNode[node.name] ?? [];
    const statusIndex: Record<string, number> = {};

    for (let i = 0; i < sessions.length; i++) {
      const session = sessions[i];
      const idx = statusIndex[session.status] ?? 0;
      statusIndex[session.status] = idx + 1;
      const existing = existingById.get(session.id);

      if (existing) {
        // Update data fields, keep position and animation state
        const statusChanged = existing.status !== session.status;
        existing.status = session.status;
        existing.ink = session.ink;
        existing.command = session.command;
        existing.description = session.description;
        existing.workdir = session.workdir;
        existing.createdAt = session.created_at;
        existing.lastOutputAt = session.last_output_at;
        existing.interventionReason = session.intervention_reason;
        existing.nodeName = node.name;
        if (statusChanged) {
          const [hx, hy] = assignHomeForStatus(node.x, session.status, idx);
          existing.homeX = hx;
          existing.homeY = hy;
        }
        newOctopuses.push(existing);
      } else {
        // New octopus — place near its node in status-appropriate zone
        const [hx, hy] = assignHomeForStatus(node.x, session.status, idx);
        newOctopuses.push({
          sessionId: session.id,
          name: session.name,
          status: session.status,
          command: session.command,
          description: session.description,
          ink: session.ink,
          workdir: session.workdir,
          createdAt: session.created_at,
          lastOutputAt: session.last_output_at,
          interventionReason: session.intervention_reason,
          nodeName: node.name,
          x: hx + randomBetween(-10, 10),
          y: hy + randomBetween(-10, 10),
          homeX: hx,
          homeY: hy,
          vx: 0,
          vy: 0,
          animFrame: 0,
          animTimer: 0,
          isSwimming: false,
          wanderTimer: randomBetween(1, 3),
          wanderTargetX: hx,
          wanderTargetY: hy,
        });
      }
    }
  }

  world.octopuses = newOctopuses;
}

// --- Physics / AI update ---

export function update(world: WorldState, dt: number): void {
  const cappedDt = Math.min(dt, 0.1);

  // Update octopuses
  for (const oct of world.octopuses) {
    const behavior = BEHAVIOR[oct.status] ?? BEHAVIOR.active;

    // Wander: pick new target periodically
    oct.wanderTimer -= cappedDt;
    if (oct.wanderTimer <= 0) {
      const angle = Math.random() * Math.PI * 2;
      oct.wanderTargetX = oct.homeX + Math.cos(angle) * behavior.radius;
      oct.wanderTargetY = oct.homeY + Math.sin(angle) * behavior.radius * 0.5;
      oct.wanderTargetY = Math.max(SWIM_ZONE_TOP, Math.min(SWIM_ZONE_BOTTOM, oct.wanderTargetY));
      oct.wanderTimer = randomBetween(behavior.intervalMin, behavior.intervalMax);
    }

    // Special status overrides
    if (oct.status === 'ready') {
      oct.wanderTargetY = Math.max(SWIM_ZONE_TOP - 20, oct.homeY - 30);
    } else if (oct.status === 'killed') {
      oct.wanderTargetY = SWIM_ZONE_BOTTOM + 10;
    }

    // Smooth movement toward target
    const dx = oct.wanderTargetX - oct.x;
    const dy = oct.wanderTargetY - oct.y;
    const lerpFactor = 2 * cappedDt * (behavior.speed / 30);
    oct.vx = dx * lerpFactor;
    oct.vy = dy * lerpFactor;
    oct.x += oct.vx;
    oct.y += oct.vy;

    // Swimming if moving horizontally fast enough
    oct.isSwimming = Math.abs(oct.vx) > 0.3;

    // Frame animation (slower for smoother feel)
    oct.animTimer += cappedDt;
    const fps = oct.isSwimming ? 5 : 3;
    if (oct.animTimer >= 1 / fps) {
      oct.animTimer = 0;
      oct.animFrame = (oct.animFrame + 1) % 4;
    }
  }

  // Separation: push overlapping octopuses apart so labels stay readable.
  // Uses elliptical distance — sprites are taller than wide (sprite + labels),
  // so vertical proximity triggers repulsion sooner than horizontal.
  for (let i = 0; i < world.octopuses.length; i++) {
    const a = world.octopuses[i];
    for (let j = i + 1; j < world.octopuses.length; j++) {
      const b = world.octopuses[j];
      const sdx = a.x - b.x;
      const sdy = a.y - b.y;
      // Shrink Y in distance calc so vertically-close octopuses appear "nearer"
      const scaledDy = sdy / SEPARATION_VERT_SCALE;
      const distSq = sdx * sdx + scaledDy * scaledDy;
      if (distSq < SEPARATION_DIST * SEPARATION_DIST && distSq > 0.01) {
        const dist = Math.sqrt(distSq);
        const push = ((SEPARATION_DIST - dist) / SEPARATION_DIST) * 30 * cappedDt;
        const realDist = Math.sqrt(sdx * sdx + sdy * sdy);
        if (realDist > 0.01) {
          const nx = sdx / realDist;
          const ny = sdy / realDist;
          // Bias vertical push so octopuses spread more in Y
          a.x += nx * push;
          a.y += ny * push * SEPARATION_VERT_SCALE;
          b.x -= nx * push;
          b.y -= ny * push * SEPARATION_VERT_SCALE;
        }
      }
    }
  }

  // Clamp octopuses to swim zone after separation
  for (const oct of world.octopuses) {
    oct.y = Math.max(SWIM_ZONE_TOP, Math.min(SEABED_Y, oct.y));
  }

  // Update bubbles
  for (let i = world.bubbles.length - 1; i >= 0; i--) {
    const b = world.bubbles[i];
    b.y -= b.speed * cappedDt;
    b.alpha -= 0.3 * cappedDt;
    if (b.alpha <= 0 || b.y < -20) {
      world.bubbles.splice(i, 1);
    }
  }

  // Spawn bubbles from octopuses or seabed
  if (world.nodes.length > 0 && Math.random() < cappedDt * 2) {
    const fromOctopus = world.octopuses.length > 0 && Math.random() > 0.3;
    let bx: number;
    let by: number;

    if (fromOctopus) {
      const src = world.octopuses[Math.floor(Math.random() * world.octopuses.length)];
      bx = src.x + randomBetween(-5, 5);
      by = src.y - 5;
    } else {
      const nodeXs = world.nodes.map((n) => n.x);
      bx = randomBetween(Math.min(...nodeXs) - 50, Math.max(...nodeXs) + 50);
      by = SEABED_Y;
    }

    world.bubbles.push({
      x: bx,
      y: by,
      radius: randomBetween(1, 3),
      speed: randomBetween(15, 30),
      alpha: randomBetween(0.3, 0.7),
    });
  }

  // Update fauna — horizontal drift + animation
  const camLeft = world.camera.x - 400;
  const camRight = world.camera.x + world.camera.width / world.camera.zoom + 400;
  for (const f of world.fauna) {
    f.x += f.vx * cappedDt;
    // Bubbles drift upward and wrap to bottom
    if (f.type === 'bubbles') {
      f.y -= 8 * cappedDt;
      if (f.y < SWIM_ZONE_TOP - 30) f.y = SWIM_ZONE_BOTTOM + 10;
    }
    // Wrap around when offscreen
    if (f.vx > 0 && f.x > camRight) f.x = camLeft - 50;
    if (f.vx < 0 && f.x < camLeft) f.x = camRight + 50;
    // Advance animation
    f.animTimer += cappedDt;
    if (f.animTimer >= f.animSpeed) {
      f.animTimer -= f.animSpeed;
      f.animFrame = (f.animFrame + 1) % 4;
    }
  }
}

// --- Hit testing ---

export function hitTest(world: WorldState, worldX: number, worldY: number): OctopusEntity | null {
  const HIT_RADIUS = 16;
  for (const oct of world.octopuses) {
    const dx = worldX - oct.x;
    const dy = worldY - oct.y;
    if (dx * dx + dy * dy < HIT_RADIUS * HIT_RADIUS) {
      return oct;
    }
  }
  return null;
}

// --- Node hit testing ---

const NODE_HIT_RX = 60;
const NODE_HIT_RY = 25;

export function hitTestNode(
  world: WorldState,
  worldX: number,
  worldY: number,
): NodeLandmark | null {
  for (const node of world.nodes) {
    const dx = (worldX - node.x) / NODE_HIT_RX;
    const dy = (worldY - node.y) / NODE_HIT_RY;
    if (dx * dx + dy * dy <= 1) {
      return node;
    }
  }
  return null;
}
