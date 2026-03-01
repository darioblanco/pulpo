import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import ChatView from './ChatView.svelte';

const mockGetSessionOutput = vi.fn();
const mockSendInput = vi.fn();

vi.mock('$lib/api', () => ({
  getSessionOutput: (...args: unknown[]) => mockGetSessionOutput(...args),
  sendInput: (...args: unknown[]) => mockSendInput(...args),
}));

beforeEach(() => {
  mockGetSessionOutput.mockReset();
  mockSendInput.mockReset();
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('ChatView', () => {
  it('loads initial output and renders agent message', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'Hello from agent' });

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(screen.getByText('Hello from agent')).toBeTruthy();
    });
  });

  it('strips ANSI escape codes from output', async () => {
    mockGetSessionOutput.mockResolvedValue({
      output: '\x1B[32mGreen text\x1B[0m',
    });

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(screen.getByText('Green text')).toBeTruthy();
    });
  });

  it('polls for new output on running sessions', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'initial' });

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(mockGetSessionOutput).toHaveBeenCalledTimes(1);
    });

    mockGetSessionOutput.mockResolvedValue({ output: 'initial more output' });
    await vi.advanceTimersByTimeAsync(2000);

    await vi.waitFor(() => {
      expect(mockGetSessionOutput).toHaveBeenCalledTimes(2);
    });
  });

  it('does not poll for non-running sessions', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'done' });

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'completed' },
    });

    await vi.waitFor(() => {
      expect(mockGetSessionOutput).toHaveBeenCalledTimes(1);
    });

    await vi.advanceTimersByTimeAsync(2000);
    expect(mockGetSessionOutput).toHaveBeenCalledTimes(1);
  });

  it('shows messagebar only for running sessions', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });

    const { unmount } = render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(screen.getByPlaceholderText('Type a message...')).toBeTruthy();
    });

    unmount();
    cleanup();

    render(ChatView, {
      props: { sessionId: 'sess-2', sessionStatus: 'completed' },
    });

    expect(screen.queryByPlaceholderText('Type a message...')).toBeNull();
  });

  it('sends input and adds user message', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'agent says hi' });
    mockSendInput.mockResolvedValue(undefined);

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(screen.getByText('agent says hi')).toBeTruthy();
    });

    const textarea = screen.getByPlaceholderText('Type a message...');
    await fireEvent.input(textarea, { target: { value: 'my command' } });

    const sendBtn = screen.getByText('Send');
    await fireEvent.click(sendBtn);

    expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'my command\n');
    expect(screen.getByText('my command')).toBeTruthy();
  });

  it('shows empty state when no output', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'completed' },
    });

    await vi.waitFor(() => {
      expect(mockGetSessionOutput).toHaveBeenCalled();
    });

    // No messages should be displayed, but the component should render
    expect(screen.queryByText('Agent')).toBeNull();
  });

  it('cleans up polling on destroy', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'test' });

    const { unmount } = render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(mockGetSessionOutput).toHaveBeenCalledTimes(1);
    });

    unmount();

    const callCount = mockGetSessionOutput.mock.calls.length;
    await vi.advanceTimersByTimeAsync(2000);
    expect(mockGetSessionOutput).toHaveBeenCalledTimes(callCount);
  });

  it('creates new agent message after user input', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'first output' });
    mockSendInput.mockResolvedValue(undefined);

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    await vi.waitFor(() => {
      expect(screen.getByText('first output')).toBeTruthy();
    });

    // Send user input
    const textarea = screen.getByPlaceholderText('Type a message...');
    await fireEvent.input(textarea, { target: { value: 'do something' } });
    await fireEvent.click(screen.getByText('Send'));

    // Next poll returns additional output
    mockGetSessionOutput.mockResolvedValue({ output: 'first output\nnew response' });
    await vi.advanceTimersByTimeAsync(2000);

    await vi.waitFor(() => {
      // The new output delta should appear as a separate agent message
      expect(screen.getByText(/new response/)).toBeTruthy();
    });
  });

  it('handles fetch error gracefully', async () => {
    mockGetSessionOutput.mockRejectedValue(new Error('network error'));

    render(ChatView, {
      props: { sessionId: 'sess-1', sessionStatus: 'running' },
    });

    // Should not crash — just no messages
    await vi.advanceTimersByTimeAsync(100);
    expect(screen.queryByText('Agent')).toBeNull();
  });
});
