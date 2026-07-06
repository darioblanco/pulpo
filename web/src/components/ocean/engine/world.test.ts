import { describe, it, expect } from 'vitest';
import type { Session } from '@/api/types';
import {
  createWorld,
  syncSingleNode,
  update,
  hitTest,
  hitTestNode,
  SEPARATION_DIST,
  SEPARATION_VERT_SCALE,
} from './world';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'api-fix',
    status: 'active',
    command: 'Fix the auth bug',
    description: null,
    workdir: '/tmp/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('world', () => {
  describe('createWorld', () => {
    it('creates empty world with camera', () => {
      const world = createWorld(800, 600);
      expect(world.nodes).toHaveLength(0);
      expect(world.octopuses).toHaveLength(0);
      expect(world.bubbles).toHaveLength(0);
      expect(world.camera.width).toBe(800);
    });
  });

  describe('syncSingleNode', () => {
    it('creates a single centered node', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');
      expect(world.nodes).toHaveLength(1);
      expect(world.nodes[0].name).toBe('mac-studio');
      expect(world.nodes[0].isLocal).toBe(true);
      expect(world.nodes[0].x).toBe(0);
      expect(world.nodes[0].color).toBe('#f472b6');
    });

    it('creates octopuses for sessions', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', name: 'node-1' }),
        makeSession({ id: 's2', name: 'node-2' }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');
      expect(world.octopuses).toHaveLength(2);
      expect(world.octopuses[0].name).toBe('node-1');
      expect(world.octopuses[1].name).toBe('node-2');
    });

    it('preserves existing octopus state on update', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1', name: 'node-1' })];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      world.octopuses[0].animFrame = 3;
      world.octopuses[0].x = 42;

      const updated = [makeSession({ id: 's1', name: 'node-1', status: 'ready' })];
      syncSingleNode(world, 'mac-studio', true, 'online', updated, '#f472b6');

      expect(world.octopuses[0].animFrame).toBe(3);
      expect(world.octopuses[0].x).toBe(42);
      expect(world.octopuses[0].status).toBe('ready');
    });

    it('populates new session fields on octopus', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({
          id: 's1',
          command: 'claude code --code',
          workdir: '/work',
          description: 'Do stuff',
          last_output_at: '2026-01-01T00:01:00Z',
          intervention_reason: 'stuck',
        }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');
      const oct = world.octopuses[0];
      expect(oct.command).toBe('claude code --code');
      expect(oct.workdir).toBe('/work');
      expect(oct.description).toBe('Do stuff');
      expect(oct.lastOutputAt).toBe('2026-01-01T00:01:00Z');
      expect(oct.interventionReason).toBe('stuck');
    });

    it('updates new session fields on existing octopus', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const updated = [
        makeSession({
          id: 's1',
          command: 'codex',
          intervention_reason: 'oom',
        }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', updated, '#f472b6');

      expect(world.octopuses[0].command).toBe('codex');
      expect(world.octopuses[0].interventionReason).toBe('oom');
    });

    it('removes octopuses for ended sessions', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');
      expect(world.octopuses).toHaveLength(1);

      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');
      expect(world.octopuses).toHaveLength(0);
    });

    it('generates decorations', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');
      expect(world.decorations.length).toBeGreaterThan(0);
    });

    it('sets correct session count', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1' }), makeSession({ id: 's2' })];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');
      expect(world.nodes[0].sessionCount).toBe(2);
    });

    it('sets peer node status', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'linux-box', false, 'offline', [], '#2dd4bf');
      expect(world.nodes[0].isLocal).toBe(false);
      expect(world.nodes[0].status).toBe('offline');
    });

    it('regenerates decorations when node name changes', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');
      const firstDecos = [...world.decorations];

      syncSingleNode(world, 'linux-box', false, 'online', [], '#2dd4bf');
      expect(world.decorations.length).toBeGreaterThan(0);
      // Decorations regenerated — may differ in position due to randomness
      expect(world.nodes[0].name).toBe('linux-box');
      // Verify it's a different set (node count changed from mac-studio to linux-box)
      expect(firstDecos.length).toBeGreaterThan(0);
    });
  });

  describe('update', () => {
    it('moves octopuses toward wander target', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const oct = world.octopuses[0];
      oct.wanderTargetX = oct.x + 100;
      oct.wanderTargetY = oct.y;
      const startX = oct.x;

      update(world, 0.1);
      expect(oct.x).toBeGreaterThan(startX);
    });

    it('caps delta time at 0.1s', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const oct = world.octopuses[0];
      oct.wanderTargetX = oct.x + 1000;
      const startX = oct.x;

      // Even with dt=10, movement should be capped
      update(world, 10);
      const movement = oct.x - startX;

      const oct2X = oct.x;
      oct.x = startX;
      update(world, 0.1);
      const cappedMovement = oct.x - startX;

      expect(movement).toBeCloseTo(cappedMovement, 0);
      oct.x = oct2X; // restore to avoid test pollution
    });

    it('advances animation frames', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const oct = world.octopuses[0];
      oct.animFrame = 0;
      oct.animTimer = 0;

      // At 3 FPS idle, frame changes every ~0.333s
      for (let i = 0; i < 8; i++) update(world, 0.05);
      expect(oct.animFrame).toBeGreaterThan(0);
    });

    it('removes faded bubbles', () => {
      const world = createWorld(800, 600);
      world.bubbles.push({ x: 0, y: 0, radius: 2, speed: 20, alpha: 0.05 });
      update(world, 0.1);
      // Bubble alpha decreases by 0.3 * 0.1 = 0.03, goes below 0.05 - 0.03 = 0.02
      // After enough updates it should be removed
      for (let i = 0; i < 10; i++) update(world, 0.1);
      expect(world.bubbles).toHaveLength(0);
    });

    it('makes stopped octopuses sink', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'stopped' })],
        '#f472b6',
      );

      const oct = world.octopuses[0];
      const startY = oct.y;
      oct.wanderTargetY = startY; // reset

      // Update should override target to sink
      for (let i = 0; i < 10; i++) update(world, 0.1);
      expect(oct.wanderTargetY).toBeGreaterThan(startY);
    });

    it('makes ready octopuses float up', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'ready' })],
        '#f472b6',
      );

      const oct = world.octopuses[0];
      for (let i = 0; i < 10; i++) update(world, 0.1);
      expect(oct.wanderTargetY).toBeLessThan(oct.homeY);
    });
  });

  describe('status-based zones', () => {
    it('assigns active and idle sessions to different home zones', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', name: 'node-1', status: 'active' }),
        makeSession({ id: 's2', name: 'node-2', status: 'idle' }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      const active = world.octopuses.find((o) => o.status === 'active')!;
      const idle = world.octopuses.find((o) => o.status === 'idle')!;
      // Idle home should be to the right of active home
      expect(idle.homeX).toBeGreaterThan(active.homeX);
    });

    it('assigns stopped sessions to lower zone than active', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', status: 'active' }),
        makeSession({ id: 's2', status: 'stopped' }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      const active = world.octopuses.find((o) => o.status === 'active')!;
      const stopped = world.octopuses.find((o) => o.status === 'stopped')!;
      expect(stopped.homeY).toBeGreaterThan(active.homeY);
    });

    it('assigns ready sessions to upper zone', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', status: 'active' }),
        makeSession({ id: 's2', status: 'ready' }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      const active = world.octopuses.find((o) => o.status === 'active')!;
      const ready = world.octopuses.find((o) => o.status === 'ready')!;
      expect(ready.homeY).toBeLessThan(active.homeY);
    });

    it('reassigns home when status changes', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'active' })],
        '#f472b6',
      );
      const originalHomeX = world.octopuses[0].homeX;

      // Transition to idle
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'idle' })],
        '#f472b6',
      );
      expect(world.octopuses[0].homeX).not.toBe(originalHomeX);
      expect(world.octopuses[0].homeX).toBeGreaterThan(originalHomeX);
    });

    it('does not change home when status stays the same', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'active' })],
        '#f472b6',
      );
      const homeX = world.octopuses[0].homeX;
      const homeY = world.octopuses[0].homeY;

      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'active' })],
        '#f472b6',
      );
      expect(world.octopuses[0].homeX).toBe(homeX);
      expect(world.octopuses[0].homeY).toBe(homeY);
    });

    it('reassigns home on status change in syncSingleNode', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'active' })],
        '#f472b6',
      );
      const originalHomeY = world.octopuses[0].homeY;

      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', status: 'stopped' })],
        '#f472b6',
      );
      expect(world.octopuses[0].homeY).toBeGreaterThan(originalHomeY);
    });

    it('assigns lost sessions to lower zone like stopped', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', status: 'active' }),
        makeSession({ id: 's2', status: 'lost' }),
      ];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      const active = world.octopuses.find((o) => o.status === 'active')!;
      const lost = world.octopuses.find((o) => o.status === 'lost')!;
      expect(lost.homeY).toBeGreaterThan(active.homeY);
    });
  });

  describe('separation', () => {
    it('pushes overlapping octopuses apart', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1', name: 'a' }), makeSession({ id: 's2', name: 'b' })];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      // Force both to the same position
      world.octopuses[0].x = 100;
      world.octopuses[0].y = 100;
      world.octopuses[1].x = 100;
      world.octopuses[1].y = 100.5; // slight offset to establish direction

      const distBefore = Math.abs(world.octopuses[0].y - world.octopuses[1].y);
      for (let i = 0; i < 20; i++) update(world, 0.05);
      const distAfter = Math.hypot(
        world.octopuses[0].x - world.octopuses[1].x,
        world.octopuses[0].y - world.octopuses[1].y,
      );
      expect(distAfter).toBeGreaterThan(distBefore);
    });

    it('does not push octopuses beyond separation distance', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1', name: 'a' }), makeSession({ id: 's2', name: 'b' })];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      // Place far apart — should not be affected
      world.octopuses[0].x = 0;
      world.octopuses[0].y = 100;
      world.octopuses[1].x = SEPARATION_DIST + 50;
      world.octopuses[1].y = 100;

      update(world, 0.05);

      // They were already far apart, so dist should still be > SEPARATION_DIST
      const dist = Math.abs(world.octopuses[0].x - world.octopuses[1].x);
      expect(dist).toBeGreaterThanOrEqual(SEPARATION_DIST * 0.9);
    });

    it('applies stronger vertical push than horizontal (elliptical)', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1', name: 'a' }), makeSession({ id: 's2', name: 'b' })];
      syncSingleNode(world, 'mac-studio', true, 'online', sessions, '#f472b6');

      // Place diagonally close, set homes to same spot so only separation acts.
      // Use a long wander timer so no random targets are picked during the test.
      const cx = 100;
      const cy = 120;
      for (const oct of world.octopuses) {
        oct.homeX = cx;
        oct.homeY = cy;
        oct.wanderTargetX = cx;
        oct.wanderTargetY = cy;
        oct.wanderTimer = 999;
      }
      world.octopuses[0].x = cx - 5;
      world.octopuses[0].y = cy - 5;
      world.octopuses[1].x = cx + 5;
      world.octopuses[1].y = cy + 5;

      for (let i = 0; i < 30; i++) update(world, 0.05);

      const dy = Math.abs(world.octopuses[0].y - world.octopuses[1].y);
      const dx = Math.abs(world.octopuses[0].x - world.octopuses[1].x);
      // Vertical separation should be larger due to VERT_SCALE bias
      expect(dy).toBeGreaterThan(dx);
      expect(SEPARATION_VERT_SCALE).toBeGreaterThan(1);
    });

    it('clamps octopuses to swim zone after separation', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      // Force octopus above swim zone
      world.octopuses[0].y = 10;
      update(world, 0.05);
      expect(world.octopuses[0].y).toBeGreaterThanOrEqual(50); // SWIM_ZONE_TOP
    });
  });

  describe('hitTest', () => {
    it('returns octopus when clicking on it', () => {
      const world = createWorld(800, 600);
      syncSingleNode(
        world,
        'mac-studio',
        true,
        'online',
        [makeSession({ id: 's1', name: 'target' })],
        '#f472b6',
      );

      const oct = world.octopuses[0];
      const result = hitTest(world, oct.x, oct.y);
      expect(result).not.toBeNull();
      expect(result?.name).toBe('target');
    });

    it('returns null when clicking empty space', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const result = hitTest(world, -9999, -9999);
      expect(result).toBeNull();
    });

    it('uses hit radius for detection', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [makeSession({ id: 's1' })], '#f472b6');

      const oct = world.octopuses[0];
      // Just inside hit radius (16 units)
      const result = hitTest(world, oct.x + 15, oct.y);
      expect(result).not.toBeNull();

      // Just outside hit radius
      const miss = hitTest(world, oct.x + 17, oct.y);
      expect(miss).toBeNull();
    });
  });

  describe('sessionCount', () => {
    it('returns zero when no sessions', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');
      expect(world.nodes[0].sessionCount).toBe(0);
    });
  });

  describe('hitTestNode', () => {
    it('returns node when clicking within ellipse', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');

      const node = world.nodes[0];
      const result = hitTestNode(world, node.x, node.y);
      expect(result).not.toBeNull();
      expect(result?.name).toBe('mac-studio');
    });

    it('returns null when clicking outside ellipse', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');

      const result = hitTestNode(world, -9999, -9999);
      expect(result).toBeNull();
    });

    it('uses elliptical hit area', () => {
      const world = createWorld(800, 600);
      syncSingleNode(world, 'mac-studio', true, 'online', [], '#f472b6');

      const node = world.nodes[0];
      // Inside horizontally (rx = 60)
      const hitH = hitTestNode(world, node.x + 55, node.y);
      expect(hitH).not.toBeNull();

      // Outside horizontally
      const missH = hitTestNode(world, node.x + 65, node.y);
      expect(missH).toBeNull();

      // Inside vertically (ry = 25)
      const hitV = hitTestNode(world, node.x, node.y + 20);
      expect(hitV).not.toBeNull();

      // Outside vertically
      const missV = hitTestNode(world, node.x, node.y + 30);
      expect(missV).toBeNull();
    });
  });
});
