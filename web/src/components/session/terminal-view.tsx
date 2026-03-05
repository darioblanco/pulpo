import { useEffect, useRef } from 'react';
import { resolveWsUrl } from '@/api/client';

interface TerminalViewProps {
  sessionId: string;
}

const TERMINAL_FONT_FAMILY = "'JetBrains Mono', 'SF Mono', 'Cascadia Code', 'Fira Code', monospace";

export function TerminalView({ sessionId }: TerminalViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const terminalRef = useRef<import('ghostty-web').Terminal | null>(null);
  const fitAddonRef = useRef<import('ghostty-web').FitAddon | null>(null);
  const receivedDataRef = useRef(false);

  useEffect(() => {
    let disposed = false;
    let observer: ResizeObserver | null = null;

    async function setup() {
      const { init, Terminal, FitAddon } = await import('ghostty-web');
      await init();

      if (disposed || !hostRef.current) return;

      const terminal = new Terminal({
        fontSize: 13,
        fontFamily: TERMINAL_FONT_FAMILY,
        theme: {
          background: '#0a1628',
          foreground: '#e0e0e0',
          cursor: '#3ee6a8',
          selectionBackground: '#2f8cff44',
        },
      });

      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminalRef.current = terminal;
      fitAddonRef.current = fitAddon;

      function openTerminal(container: HTMLElement) {
        if (disposed || wsRef.current) return;

        terminal.open(container);
        fitAddon.fit();

        const ws = new WebSocket(resolveWsUrl(`/api/v1/sessions/${sessionId}/stream`));
        ws.binaryType = 'arraybuffer';
        wsRef.current = ws;

        ws.onmessage = (event) => {
          receivedDataRef.current = true;
          if (event.data instanceof ArrayBuffer) {
            terminal.write(new Uint8Array(event.data));
          } else {
            terminal.write(event.data);
          }
        };

        ws.onclose = () => {
          if (!receivedDataRef.current && !disposed) {
            terminal.writeln('\r\n\x1b[33mDisconnected from session.\x1b[0m');
          }
        };

        ws.onerror = () => {
          terminal.writeln('\r\n\x1b[31mWebSocket error.\x1b[0m');
        };

        terminal.onData((data) => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(new TextEncoder().encode(data));
          }
        });

        terminal.onResize(({ cols, rows }) => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify({ type: 'resize', cols, rows }));
          }
        });
      }

      // Keep terminal dimensions synchronized with container size.
      // Tabs/sidebar/mobile layout changes can otherwise leave ghostty overflowing.
      observer = new ResizeObserver((entries) => {
        const entry = entries[0];
        if (!entry || disposed || !hostRef.current) return;
        const { width, height } = entry.contentRect;
        if (width <= 0 || height <= 0) return;

        if (!wsRef.current) {
          openTerminal(hostRef.current);
        } else {
          fitAddonRef.current?.fit();
        }
      });
      observer.observe(hostRef.current);
    }

    setup();

    return () => {
      disposed = true;
      observer?.disconnect();
      wsRef.current?.close();
      fitAddonRef.current?.dispose();
      terminalRef.current?.dispose();
    };
  }, [sessionId]);

  return (
    <div
      data-testid="terminal-view"
      ref={containerRef}
      className="my-2 w-full min-w-0 overflow-hidden rounded-lg border border-border bg-background"
    >
      <div ref={hostRef} className="h-[clamp(220px,45vh,560px)] w-full min-w-0 overflow-hidden" />
    </div>
  );
}
