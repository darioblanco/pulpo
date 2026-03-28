import { useState, useEffect, useRef, useCallback } from 'react';
import type { Session, NodeInfo, PeerInfo } from '@/api/types';
import { loadAllSprites, type Sprites } from './engine/sprites';
import {
  createWorld,
  syncData,
  update,
  hitTest,
  hitTestNode,
  type WorldState,
  type OctopusEntity,
  type NodeLandmark,
} from './engine/world';
import { render } from './engine/renderer';
import { screenToWorld } from './engine/camera';
import { fitCamera } from './engine/camera';
import { ProfileCard } from './profile-card';
import { NodeCard } from './node-card';

interface OceanCanvasProps {
  localNode: NodeInfo;
  localSessions: Session[];
  peers: PeerInfo[];
  peerSessions: Record<string, Session[]>;
}

export function OceanCanvas({ localNode, localSessions, peers, peerSessions }: OceanCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const worldRef = useRef<WorldState | null>(null);
  const spritesRef = useRef<Sprites | null>(null);
  const rafRef = useRef<number>(0);

  const [selectedOctopus, setSelectedOctopus] = useState<{
    entity: OctopusEntity;
    screenX: number;
    screenY: number;
  } | null>(null);
  const [selectedNode, setSelectedNode] = useState<{
    entity: NodeLandmark;
    screenX: number;
    screenY: number;
  } | null>(null);
  const [loading, setLoading] = useState(true);

  // Load sprites once
  useEffect(() => {
    loadAllSprites()
      .then((sprites) => {
        spritesRef.current = sprites;
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  // Initialize canvas + resize handler
  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    const resize = () => {
      const rect = container.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;

      const dpr = window.devicePixelRatio || 1;
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      canvas.style.width = `${rect.width}px`;
      canvas.style.height = `${rect.height}px`;

      if (!worldRef.current) {
        worldRef.current = createWorld(rect.width, rect.height);
      } else {
        worldRef.current.camera.width = rect.width;
        worldRef.current.camera.height = rect.height;
        fitCamera(worldRef.current.camera, worldRef.current.nodes);
      }
    };

    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  // Sync React data into world
  useEffect(() => {
    if (!worldRef.current) return;
    syncData(worldRef.current, localNode, localSessions, peers, peerSessions);
  }, [localNode, localSessions, peers, peerSessions]);

  // Game loop
  useEffect(() => {
    if (loading) return;

    let lastTime = performance.now();

    const loop = (now: number) => {
      const dt = (now - lastTime) / 1000;
      lastTime = now;

      const world = worldRef.current;
      const sprites = spritesRef.current;
      const canvas = canvasRef.current;

      if (world && sprites && canvas) {
        update(world, dt);

        const ctx = canvas.getContext('2d');
        if (ctx) {
          const dpr = window.devicePixelRatio || 1;
          ctx.save();
          ctx.scale(dpr, dpr);
          render(ctx, world, sprites, now);
          ctx.restore();
        }
      }

      rafRef.current = requestAnimationFrame(loop);
    };

    rafRef.current = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafRef.current);
  }, [loading]);

  // Click → hit test → profile card
  const handleClick = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const world = worldRef.current;
    const canvas = canvasRef.current;
    if (!world || !canvas) return;

    const rect = canvas.getBoundingClientRect();
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const [wx, wy] = screenToWorld(world.camera, sx, sy);
    const oct = hitTest(world, wx, wy);

    if (oct) {
      setSelectedOctopus({ entity: oct, screenX: e.clientX, screenY: e.clientY });
      setSelectedNode(null);
    } else {
      setSelectedOctopus(null);
      const node = hitTestNode(world, wx, wy);
      if (node) {
        setSelectedNode({ entity: node, screenX: e.clientX, screenY: e.clientY });
      } else {
        setSelectedNode(null);
      }
    }
  }, []);

  return (
    <div
      ref={containerRef}
      className="relative w-full"
      style={{ aspectRatio: '16 / 9', maxHeight: 'calc(100dvh - 8rem)' }}
      data-testid="ocean-canvas-container"
    >
      <canvas
        ref={canvasRef}
        data-testid="ocean-canvas"
        className="block w-full h-full cursor-pointer"
        onClick={handleClick}
      />
      {selectedOctopus && (
        <ProfileCard
          octopus={selectedOctopus.entity}
          screenX={selectedOctopus.screenX}
          screenY={selectedOctopus.screenY}
          onClose={() => setSelectedOctopus(null)}
        />
      )}
      {selectedNode && (
        <NodeCard
          node={selectedNode.entity}
          screenX={selectedNode.screenX}
          screenY={selectedNode.screenY}
          onClose={() => setSelectedNode(null)}
        />
      )}
      {loading && (
        <div
          className="absolute inset-0 flex items-center justify-center"
          data-testid="loading-overlay"
        >
          <span className="font-mono text-muted-foreground">Loading sprites...</span>
        </div>
      )}
    </div>
  );
}
