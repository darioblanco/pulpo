import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import TerminalComponent from './Terminal.svelte';

// Use vi.hoisted to define mocks before vi.mock hoisting
const {
  mockWrite,
  mockWriteln,
  mockOpen,
  mockDispose,
  mockLoadAddon,
  mockOnData,
  mockOnResize,
  mockFit,
  mockResolveWsUrl,
} = vi.hoisted(() => ({
  mockWrite: vi.fn(),
  mockWriteln: vi.fn(),
  mockOpen: vi.fn(),
  mockDispose: vi.fn(),
  mockLoadAddon: vi.fn(),
  mockOnData: vi.fn(),
  mockOnResize: vi.fn(),
  mockFit: vi.fn(),
  mockResolveWsUrl: vi.fn(),
}));

vi.mock('@xterm/xterm', () => ({
  Terminal: class MockTerminal {
    write = mockWrite;
    writeln = mockWriteln;
    open = mockOpen;
    dispose = mockDispose;
    loadAddon = mockLoadAddon;
    onData = mockOnData;
    onResize = mockOnResize;
  },
}));

vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class MockFitAddon {
    fit = mockFit;
  },
}));

// Mock xterm CSS import
vi.mock('@xterm/xterm/css/xterm.css', () => ({}));

// Mock resolveWsUrl from api module
vi.mock('$lib/api', () => ({
  resolveWsUrl: (path: string) => mockResolveWsUrl(path),
}));

// Mock WebSocket
let wsInstances: MockWebSocket[] = [];

class MockWebSocket {
  static OPEN = 1;
  url: string;
  binaryType = '';
  readyState = 1; // OPEN
  onopen: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: unknown) => void) | null = null;
  onclose: ((ev: unknown) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  send = vi.fn();
  close = vi.fn();

  constructor(url: string) {
    this.url = url;
    wsInstances.push(this);
  }
}

vi.stubGlobal('WebSocket', MockWebSocket);

// Mock ResizeObserver — capture callbacks for testing
const mockObserve = vi.fn();
let resizeObserverCallbacks: (() => void)[] = [];

vi.stubGlobal(
  'ResizeObserver',
  class {
    observe = mockObserve;
    unobserve = vi.fn();
    disconnect = vi.fn();
    private callback: () => void;
    constructor(callback: () => void) {
      this.callback = callback;
      resizeObserverCallbacks.push(callback);
    }
  },
);

afterEach(cleanup);

beforeEach(() => {
  wsInstances = [];
  resizeObserverCallbacks = [];
  mockWrite.mockReset();
  mockWriteln.mockReset();
  mockOpen.mockReset();
  mockDispose.mockReset();
  mockLoadAddon.mockReset();
  mockOnData.mockReset();
  mockOnResize.mockReset();
  mockFit.mockReset();
  mockObserve.mockReset();
  mockResolveWsUrl.mockReset();
  mockResolveWsUrl.mockImplementation((path: string) => `ws://localhost:7433${path}`);
});

describe('Terminal', () => {
  it('creates Terminal with correct config and opens it', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    expect(mockOpen).toHaveBeenCalled();
    expect(mockLoadAddon).toHaveBeenCalled();
    expect(mockFit).toHaveBeenCalled();
  });

  it('calls resolveWsUrl with correct path', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    expect(mockResolveWsUrl).toHaveBeenCalledWith('/api/v1/sessions/sess-1/stream');
    expect(wsInstances.length).toBeGreaterThan(0);
    expect(wsInstances[0].url).toBe('ws://localhost:7433/api/v1/sessions/sess-1/stream');
  });

  it('uses URL returned by resolveWsUrl (e.g. with token)', () => {
    mockResolveWsUrl.mockReturnValue('wss://remote:7433/api/v1/sessions/sess-2/stream?token=abc');

    render(TerminalComponent, { props: { sessionId: 'sess-2' } });

    const ws = wsInstances[wsInstances.length - 1];
    expect(ws.url).toBe('wss://remote:7433/api/v1/sessions/sess-2/stream?token=abc');
  });

  it('WebSocket onmessage writes binary data to terminal', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    const data = new ArrayBuffer(4);
    ws.onmessage?.({ data });

    expect(mockWrite).toHaveBeenCalledWith(expect.any(Uint8Array));
  });

  it('WebSocket onmessage writes text data to terminal', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    ws.onmessage?.({ data: 'hello' });

    expect(mockWrite).toHaveBeenCalledWith('hello');
  });

  it('terminal onData sends keystrokes via WebSocket', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const onDataCallback = mockOnData.mock.calls[0][0];
    const ws = wsInstances[0];

    onDataCallback('a');

    expect(ws.send).toHaveBeenCalled();
  });

  it('terminal onResize sends JSON resize event', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const onResizeCallback = mockOnResize.mock.calls[0][0];
    const ws = wsInstances[0];

    onResizeCallback({ cols: 80, rows: 24 });

    expect(ws.send).toHaveBeenCalledWith(JSON.stringify({ type: 'resize', cols: 80, rows: 24 }));
  });

  it('cleans up on destroy (ws.close, terminal.dispose)', () => {
    const { unmount } = render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    unmount();

    expect(ws.close).toHaveBeenCalled();
    expect(mockDispose).toHaveBeenCalled();
  });

  it('WebSocket onopen writes connected message', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    ws.onopen?.({});

    expect(mockWriteln).toHaveBeenCalledWith('\x1b[32mConnected to session.\x1b[0m');
  });

  it('WebSocket onclose writes disconnected message', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    ws.onclose?.({});

    expect(mockWriteln).toHaveBeenCalledWith('\r\n\x1b[33mDisconnected from session.\x1b[0m');
  });

  it('WebSocket onerror writes error message', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    const ws = wsInstances[0];
    ws.onerror?.({});

    expect(mockWriteln).toHaveBeenCalledWith('\r\n\x1b[31mWebSocket error.\x1b[0m');
  });

  it('ResizeObserver callback calls fitAddon.fit()', () => {
    render(TerminalComponent, { props: { sessionId: 'sess-1' } });

    // fitAddon.fit() is called once on mount
    expect(mockFit).toHaveBeenCalledTimes(1);

    // Trigger the ResizeObserver callback
    resizeObserverCallbacks[0]();

    expect(mockFit).toHaveBeenCalledTimes(2);
  });
});
