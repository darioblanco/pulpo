import { useState, useEffect, useRef, useCallback } from 'react';
import type { Session } from '@/api/types';
import { loadBackgroundSet, type Sprites, type BackgroundSprites } from './engine/sprites';
import {
  createWorld,
  syncSingleNode,
  update,
  hitTest,
  hitTestNode,
  type WorldState,
  type OctopusEntity,
  type NodeLandmark,
} from './engine/world';
import { render } from './engine/renderer';
import { screenToWorld, fitCamera } from './engine/camera';
import { ProfileCard } from './profile-card';
import { NodeCard } from './node-card';
import { AttachModal } from './attach-modal';

interface TidePoolProps {
  nodeName: string;
  isLocal: boolean;
  nodeStatus: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  backgroundIndex: number;
  nodeColor: string;
  sprites: Sprites | null;
  onKillSession?: (sessionName: string) => void;
  onDeleteSession?: (sessionName: string) => void;
}

export function TidePool({
  nodeName,
  isLocal,
  nodeStatus,
  sessions,
  backgroundIndex,
  nodeColor,
  sprites,
  onKillSession,
  onDeleteSession,
}: TidePoolProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const worldRef = useRef<WorldState | null>(null);
  const bgRef = useRef<BackgroundSprites | null>(null);
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
  const [bgLoading, setBgLoading] = useState(true);
  const [attachSession, setAttachSession] = useState<{
    name: string;
    id: string;
    status: string;
  } | null>(null);

  // Load background sprites for this pool
  useEffect(() => {
    loadBackgroundSet(backgroundIndex)
      .then((bg) => {
        bgRef.current = bg;
        setBgLoading(false);
      })
      .catch(() => setBgLoading(false));
  }, [backgroundIndex]);

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
    syncSingleNode(worldRef.current, nodeName, isLocal, nodeStatus, sessions, nodeColor);
  }, [nodeName, isLocal, nodeStatus, sessions, nodeColor]);

  // Game loop
  useEffect(() => {
    if (bgLoading || !sprites) return;

    let lastTime = performance.now();

    const loop = (now: number) => {
      const dt = (now - lastTime) / 1000;
      lastTime = now;

      const world = worldRef.current;
      const canvas = canvasRef.current;

      if (world && sprites && canvas) {
        update(world, dt);

        const ctx = canvas.getContext('2d');
        if (ctx) {
          const dpr = window.devicePixelRatio || 1;
          ctx.save();
          ctx.scale(dpr, dpr);
          render(ctx, world, sprites, now, bgRef.current ?? undefined);
          ctx.restore();
        }
      }

      rafRef.current = requestAnimationFrame(loop);
    };

    rafRef.current = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafRef.current);
  }, [bgLoading, sprites]);

  // Click -> hit test -> profile card / node card
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

  const handleAttach = useCallback(
    (sessionName: string) => {
      // Find the session to get its id and status
      const oct = selectedOctopus?.entity;
      if (oct && oct.name === sessionName) {
        setAttachSession({ name: oct.name, id: oct.sessionId, status: oct.status });
        setSelectedOctopus(null);
      }
    },
    [selectedOctopus],
  );

  const handleKill = useCallback(
    (sessionName: string) => {
      setSelectedOctopus(null);
      onKillSession?.(sessionName);
    },
    [onKillSession],
  );

  const handleDelete = useCallback(
    (sessionName: string) => {
      setSelectedOctopus(null);
      onDeleteSession?.(sessionName);
    },
    [onDeleteSession],
  );

  const statusColor =
    nodeStatus === 'online' ? '#34d399' : nodeStatus === 'offline' ? '#f87171' : '#94a3b8';

  const loading = bgLoading || !sprites;

  return (
    <div data-testid="tide-pool" className="flex flex-col">
      {/* HTML header for crisp text */}
      <div
        className="flex items-center gap-2 px-3 py-2 font-mono text-sm"
        data-testid="tide-pool-header"
      >
        <span
          className="inline-block h-2.5 w-2.5 rounded-full"
          style={{ backgroundColor: statusColor }}
          data-testid="tide-pool-status-dot"
        />
        <span className="font-bold text-white" style={{ color: nodeColor }}>
          {nodeName}
        </span>
        {isLocal && <span className="text-xs text-muted-foreground">(local)</span>}
        <span className="text-xs text-muted-foreground">
          {sessions.length} session{sessions.length !== 1 ? 's' : ''}
        </span>
      </div>

      {/* Canvas container */}
      <div
        ref={containerRef}
        className="relative w-full border border-border rounded-lg overflow-hidden"
        style={{ aspectRatio: '16 / 9' }}
        data-testid="tide-pool-canvas-container"
      >
        <canvas
          ref={canvasRef}
          data-testid="tide-pool-canvas"
          className="block w-full h-full cursor-pointer"
          onClick={handleClick}
        />
        {selectedOctopus && (
          <ProfileCard
            octopus={selectedOctopus.entity}
            screenX={selectedOctopus.screenX}
            screenY={selectedOctopus.screenY}
            onClose={() => setSelectedOctopus(null)}
            onAttach={handleAttach}
            onKill={onKillSession ? handleKill : undefined}
            onDelete={onDeleteSession ? handleDelete : undefined}
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
            data-testid="tide-pool-loading"
          >
            <span className="font-mono text-muted-foreground">Loading...</span>
          </div>
        )}
      </div>

      {/* Attach modal */}
      {attachSession && (
        <AttachModal
          sessionName={attachSession.name}
          sessionId={attachSession.id}
          sessionStatus={attachSession.status}
          open={true}
          onOpenChange={(open) => {
            if (!open) setAttachSession(null);
          }}
        />
      )}
    </div>
  );
}
