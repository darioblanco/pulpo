import { describe, it, expect } from 'vitest';
import type { Session, NodeInfo, PeerInfo } from '@/api/types';
import { createWorld, syncData, update, hitTest } from './world';

function makeNode(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return {
    name: 'mac-studio',
    hostname: 'mac-studio.local',
    os: 'macos',
    arch: 'aarch64',
    cpus: 12,
    memory_mb: 32768,
    gpu: null,
    ...overrides,
  };
}

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'api-fix',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the auth bug',
    mode: 'autonomous',
    workdir: '/tmp/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    ink: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    waiting_for_input: false,
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

  describe('syncData', () => {
    it('creates local node', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [], [], {});
      expect(world.nodes).toHaveLength(1);
      expect(world.nodes[0].name).toBe('mac-studio');
      expect(world.nodes[0].isLocal).toBe(true);
    });

    it('creates peer nodes', () => {
      const world = createWorld(800, 600);
      const peers: PeerInfo[] = [
        {
          name: 'linux-box',
          address: '10.0.0.2:7433',
          status: 'online',
          node_info: null,
          session_count: null,
        },
      ];
      syncData(world, makeNode(), [], peers, {});
      expect(world.nodes).toHaveLength(2);
      expect(world.nodes[1].name).toBe('linux-box');
      expect(world.nodes[1].isLocal).toBe(false);
      expect(world.nodes[1].status).toBe('online');
    });

    it('creates octopuses for local sessions', () => {
      const world = createWorld(800, 600);
      const sessions = [
        makeSession({ id: 's1', name: 'worker-1' }),
        makeSession({ id: 's2', name: 'worker-2' }),
      ];
      syncData(world, makeNode(), sessions, [], {});
      expect(world.octopuses).toHaveLength(2);
      expect(world.octopuses[0].name).toBe('worker-1');
      expect(world.octopuses[1].name).toBe('worker-2');
    });

    it('creates octopuses for peer sessions', () => {
      const world = createWorld(800, 600);
      const peers: PeerInfo[] = [
        {
          name: 'linux-box',
          address: '10.0.0.2:7433',
          status: 'online',
          node_info: null,
          session_count: 1,
        },
      ];
      const peerSessions = {
        'linux-box': [makeSession({ id: 'p1', name: 'peer-task', provider: 'codex' })],
      };
      syncData(world, makeNode(), [], peers, peerSessions);
      expect(world.octopuses).toHaveLength(1);
      expect(world.octopuses[0].name).toBe('peer-task');
      expect(world.octopuses[0].provider).toBe('codex');
      expect(world.octopuses[0].nodeName).toBe('linux-box');
    });

    it('preserves existing octopus animation state on update', () => {
      const world = createWorld(800, 600);
      const sessions = [makeSession({ id: 's1', name: 'worker-1' })];
      syncData(world, makeNode(), sessions, [], {});

      // Modify animation state
      world.octopuses[0].animFrame = 3;
      world.octopuses[0].x = 42;

      // Sync again with updated status
      const updated = [makeSession({ id: 's1', name: 'worker-1', status: 'completed' })];
      syncData(world, makeNode(), updated, [], {});

      expect(world.octopuses[0].animFrame).toBe(3);
      expect(world.octopuses[0].x).toBe(42);
      expect(world.octopuses[0].status).toBe('completed');
    });

    it('removes octopuses for ended sessions', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});
      expect(world.octopuses).toHaveLength(1);

      syncData(world, makeNode(), [], [], {});
      expect(world.octopuses).toHaveLength(0);
    });

    it('generates decorations', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [], [], {});
      expect(world.decorations.length).toBeGreaterThan(0);
    });

    it('regenerates decorations when nodes change', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [], [], {});
      const firstDecos = [...world.decorations];

      const peers: PeerInfo[] = [
        {
          name: 'new-peer',
          address: '10.0.0.3:7433',
          status: 'online',
          node_info: null,
          session_count: null,
        },
      ];
      syncData(world, makeNode(), [], peers, {});
      // Decorations regenerated (different length or positions)
      expect(world.decorations.length).toBeGreaterThan(0);
      expect(world.decorations.length).not.toBe(firstDecos.length);
    });
  });

  describe('update', () => {
    it('moves octopuses toward wander target', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});

      const oct = world.octopuses[0];
      oct.wanderTargetX = oct.x + 100;
      oct.wanderTargetY = oct.y;
      const startX = oct.x;

      update(world, 0.1);
      expect(oct.x).toBeGreaterThan(startX);
    });

    it('caps delta time at 0.1s', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});

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
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});

      const oct = world.octopuses[0];
      oct.animFrame = 0;
      oct.animTimer = 0;

      // At 6 FPS idle, frame changes every ~0.167s
      for (let i = 0; i < 5; i++) update(world, 0.05);
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

    it('makes dead octopuses sink', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1', status: 'dead' })], [], {});

      const oct = world.octopuses[0];
      const startY = oct.y;
      oct.wanderTargetY = startY; // reset

      // Update should override target to sink
      for (let i = 0; i < 10; i++) update(world, 0.1);
      expect(oct.wanderTargetY).toBeGreaterThan(startY);
    });

    it('makes completed octopuses float up', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1', status: 'completed' })], [], {});

      const oct = world.octopuses[0];
      for (let i = 0; i < 10; i++) update(world, 0.1);
      expect(oct.wanderTargetY).toBeLessThan(oct.homeY);
    });
  });

  describe('hitTest', () => {
    it('returns octopus when clicking on it', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1', name: 'target' })], [], {});

      const oct = world.octopuses[0];
      const result = hitTest(world, oct.x, oct.y);
      expect(result).not.toBeNull();
      expect(result?.name).toBe('target');
    });

    it('returns null when clicking empty space', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});

      const result = hitTest(world, -9999, -9999);
      expect(result).toBeNull();
    });

    it('uses hit radius for detection', () => {
      const world = createWorld(800, 600);
      syncData(world, makeNode(), [makeSession({ id: 's1' })], [], {});

      const oct = world.octopuses[0];
      // Just inside hit radius (16 units)
      const result = hitTest(world, oct.x + 15, oct.y);
      expect(result).not.toBeNull();

      // Just outside hit radius
      const miss = hitTest(world, oct.x + 17, oct.y);
      expect(miss).toBeNull();
    });
  });
});
