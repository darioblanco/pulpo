<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import '@xterm/xterm/css/xterm.css';
  import { resolveWsUrl } from '$lib/api';

  let { sessionId }: { sessionId: string } = $props();

  let terminalEl: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let ws: WebSocket | null = null;

  function connect() {
    ws = new WebSocket(resolveWsUrl(`/api/v1/sessions/${sessionId}/stream`));
    ws.binaryType = 'arraybuffer';

    ws.onopen = () => {
      terminal?.writeln('\x1b[32mConnected to session.\x1b[0m');
    };

    ws.onmessage = (event) => {
      if (event.data instanceof ArrayBuffer) {
        terminal?.write(new Uint8Array(event.data));
      } else {
        terminal?.write(event.data);
      }
    };

    ws.onclose = () => {
      terminal?.writeln('\r\n\x1b[33mDisconnected from session.\x1b[0m');
    };

    ws.onerror = () => {
      terminal?.writeln('\r\n\x1b[31mWebSocket error.\x1b[0m');
    };
  }

  onMount(() => {
    terminal = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: 'monospace',
      theme: {
        background: '#1a1a1a',
        foreground: '#e0e0e0',
        cursor: '#00ff88',
        selectionBackground: '#33ff8844',
      },
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(terminalEl);
    fitAddon.fit();

    // Send keystrokes to server
    terminal.onData((data) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        const encoder = new TextEncoder();
        ws.send(encoder.encode(data));
      }
    });

    // Send resize events
    terminal.onResize(({ cols, rows }) => {
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ type: 'resize', cols, rows }));
      }
    });

    // Handle window resize
    const resizeObserver = new ResizeObserver(() => {
      fitAddon?.fit();
    });
    resizeObserver.observe(terminalEl);

    connect();
  });

  onDestroy(() => {
    ws?.close();
    terminal?.dispose();
  });
</script>

<div class="w-full h-[300px] my-2 rounded-lg overflow-hidden" bind:this={terminalEl}></div>
