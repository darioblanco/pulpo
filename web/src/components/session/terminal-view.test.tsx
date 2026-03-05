import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TerminalView } from './terminal-view';

// Mock ghostty-web terminal
const mockTerminal = {
  loadAddon: vi.fn(),
  open: vi.fn(),
  write: vi.fn(),
  writeln: vi.fn(),
  onData: vi.fn(),
  onResize: vi.fn(),
  dispose: vi.fn(),
};

const mockFitAddon = {
  fit: vi.fn(),
  observeResize: vi.fn(),
  dispose: vi.fn(),
};

const mockInit = vi.fn().mockResolvedValue(undefined);

vi.mock('ghostty-web', () => ({
  init: (...args: unknown[]) => mockInit(...args),
  Terminal: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    Object.assign(this, { ...mockTerminal });
  }),
  FitAddon: vi.fn().mockImplementation(function (this: Record<string, unknown>) {
    Object.assign(this, { ...mockFitAddon });
  }),
}));

vi.mock('@/api/client', () => ({
  resolveWsUrl: vi.fn().mockReturnValue('ws://localhost:7433/api/v1/sessions/sess-1/stream'),
  setApiConfig: vi.fn(),
}));

// Mock WebSocket
class MockWebSocket {
  url: string;
  binaryType = 'blob';
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }

  send = vi.fn();
  close = vi.fn();

  static OPEN = 1;
  static instances: MockWebSocket[] = [];
  static reset() {
    MockWebSocket.instances = [];
  }
}

vi.stubGlobal('WebSocket', MockWebSocket);

// Mock ResizeObserver — captures the callback so tests can fire it
type ROCallback = (entries: { contentRect: { width: number; height: number } }[]) => void;
let resizeObserverCallback: ROCallback | null = null;

vi.stubGlobal(
  'ResizeObserver',
  vi.fn().mockImplementation(function (this: Record<string, unknown>, cb: ROCallback) {
    Object.assign(this, {
      observe: vi.fn().mockImplementation(() => {
        resizeObserverCallback = cb;
      }),
      disconnect: vi.fn(),
    });
  }),
);

beforeEach(() => {
  MockWebSocket.reset();
  resizeObserverCallback = null;
  Object.values(mockTerminal).forEach((fn) => fn.mockClear());
  Object.values(mockFitAddon).forEach((fn) => fn.mockClear());
  mockInit.mockClear();
});

/** Helper: wait for async setup to complete (import + init) */
async function waitForInit() {
  await vi.waitFor(() => {
    expect(mockInit).toHaveBeenCalled();
  });
}

/** Simulate ResizeObserver firing with given dimensions */
function fireResize(width: number, height: number) {
  resizeObserverCallback?.([{ contentRect: { width, height } }]);
}

describe('TerminalView', () => {
  it('renders the terminal container', () => {
    render(<TerminalView sessionId="sess-1" />);
    expect(screen.getByTestId('terminal-view')).toBeInTheDocument();
  });

  it('calls init() to load WASM before creating terminal', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await vi.waitFor(() => {
      expect(mockInit).toHaveBeenCalledTimes(1);
    });
  });

  it('loads FitAddon into terminal', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    expect(mockTerminal.loadAddon).toHaveBeenCalled();
  });

  it('skips open when container reports zero dimensions', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(0, 0);
    expect(mockTerminal.open).not.toHaveBeenCalled();
    expect(MockWebSocket.instances.length).toBe(0);
  });

  it('opens terminal and fits when container becomes visible', async () => {
    render(<TerminalView sessionId="sess-1" />);
    const container = screen.getByTestId('terminal-view');

    await waitForInit();
    fireResize(800, 400);

    expect(mockTerminal.open).toHaveBeenCalledWith(container.firstElementChild);
    expect(mockFitAddon.fit).toHaveBeenCalled();
    expect(MockWebSocket.instances.length).toBe(1);
    expect(MockWebSocket.instances[0].url).toContain('sess-1');
  });

  it('cleans up terminal and FitAddon on unmount', async () => {
    const { unmount } = render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    const ws = MockWebSocket.instances[0];
    unmount();
    expect(ws.close).toHaveBeenCalled();
    expect(mockFitAddon.dispose).toHaveBeenCalled();
  });

  it('handles WebSocket open event', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    MockWebSocket.instances[0].onopen?.();
    expect(mockTerminal.writeln).toHaveBeenCalledWith(expect.stringContaining('Connected'));
  });

  it('handles WebSocket text message', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    MockWebSocket.instances[0].onmessage?.({ data: 'hello' });
    expect(mockTerminal.write).toHaveBeenCalledWith('hello');
  });

  it('handles WebSocket binary message', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    const buf = new ArrayBuffer(4);
    MockWebSocket.instances[0].onmessage?.({ data: buf });
    expect(mockTerminal.write).toHaveBeenCalledWith(expect.any(Uint8Array));
  });

  it('handles WebSocket close event', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    MockWebSocket.instances[0].onclose?.();
    expect(mockTerminal.writeln).toHaveBeenCalledWith(expect.stringContaining('Disconnected'));
  });

  it('handles WebSocket error event', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    MockWebSocket.instances[0].onerror?.();
    expect(mockTerminal.writeln).toHaveBeenCalledWith(expect.stringContaining('error'));
  });

  it('sends terminal data through WebSocket', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    const onDataCall = mockTerminal.onData.mock.calls[0];
    expect(onDataCall).toBeDefined();
    onDataCall[0]('test-input');
    expect(MockWebSocket.instances[0].send).toHaveBeenCalled();
  });

  it('sends resize events through WebSocket', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);

    const onResizeCall = mockTerminal.onResize.mock.calls[0];
    expect(onResizeCall).toBeDefined();
    onResizeCall[0]({ cols: 80, rows: 24 });
    expect(MockWebSocket.instances[0].send).toHaveBeenCalledWith(
      JSON.stringify({ type: 'resize', cols: 80, rows: 24 }),
    );
  });

  it('does not open terminal twice on repeated resize events', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);
    fireResize(900, 500);

    expect(mockTerminal.open).toHaveBeenCalledTimes(1);
    expect(MockWebSocket.instances.length).toBe(1);
  });

  it('refits terminal when container resizes after opening', async () => {
    render(<TerminalView sessionId="sess-1" />);
    await waitForInit();
    fireResize(800, 400);
    fireResize(900, 500);

    expect(mockFitAddon.fit).toHaveBeenCalledTimes(2);
  });
});
