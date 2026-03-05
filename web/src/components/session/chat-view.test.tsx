import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { ChatView } from './chat-view';
import * as api from '@/api/client';

vi.mock('@/api/client', () => ({
  getSessionOutput: vi.fn(),
  sendInput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockGetSessionOutput = vi.mocked(api.getSessionOutput);
const mockSendInput = vi.mocked(api.sendInput);

beforeEach(() => {
  mockGetSessionOutput.mockReset();
  mockSendInput.mockReset();
});

describe('ChatView', () => {
  it('renders chat container', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<ChatView sessionId="sess-1" sessionStatus="completed" />);
    expect(screen.getByTestId('chat-view')).toBeInTheDocument();
  });

  it('fetches and displays output', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'Hello from agent' });
    render(<ChatView sessionId="sess-1" sessionStatus="completed" />);
    await waitFor(() => {
      expect(screen.getByText('Hello from agent')).toBeInTheDocument();
    });
  });

  it('strips ANSI codes from output', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '\x1B[32mGreen text\x1B[0m' });
    render(<ChatView sessionId="sess-1" sessionStatus="completed" />);
    await waitFor(() => {
      expect(screen.getByText('Green text')).toBeInTheDocument();
    });
  });

  it('shows input field for running sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<ChatView sessionId="sess-1" sessionStatus="running" />);
    expect(screen.getByTestId('chat-input')).toBeInTheDocument();
    expect(screen.getByText('Send')).toBeInTheDocument();
  });

  it('hides input field for non-running sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<ChatView sessionId="sess-1" sessionStatus="completed" />);
    expect(screen.queryByTestId('chat-input')).not.toBeInTheDocument();
  });

  it('sends input on button click', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    mockSendInput.mockResolvedValue(undefined);
    render(<ChatView sessionId="sess-1" sessionStatus="running" />);

    const input = screen.getByTestId('chat-input');
    fireEvent.change(input, { target: { value: 'Hello' } });
    fireEvent.click(screen.getByText('Send'));

    await waitFor(() => {
      expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'Hello\n');
    });
    expect(screen.getByText('Hello')).toBeInTheDocument();
  });

  it('sends input on Enter key', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    mockSendInput.mockResolvedValue(undefined);
    render(<ChatView sessionId="sess-1" sessionStatus="running" />);

    const input = screen.getByTestId('chat-input');
    fireEvent.change(input, { target: { value: 'Test' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'Test\n');
    });
  });

  it('does not send empty input', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<ChatView sessionId="sess-1" sessionStatus="running" />);

    fireEvent.click(screen.getByText('Send'));
    expect(mockSendInput).not.toHaveBeenCalled();
  });

  it('appends to existing agent message on subsequent fetches', async () => {
    let callCount = 0;
    mockGetSessionOutput.mockImplementation(async () => {
      callCount++;
      if (callCount === 1) return { output: 'Part 1' };
      return { output: 'Part 1Part 2' };
    });

    // Use running status to trigger 2s polling interval
    render(<ChatView sessionId="sess-1" sessionStatus="running" />);

    await waitFor(() => {
      expect(screen.getByText('Part 1')).toBeInTheDocument();
    });

    // Wait for the interval to fire (2s + buffer)
    await waitFor(
      () => {
        expect(screen.getByText('Part 1Part 2')).toBeInTheDocument();
      },
      { timeout: 4000 },
    );
  });

  it('handles fetch errors silently', async () => {
    mockGetSessionOutput.mockRejectedValue(new Error('Network error'));
    render(<ChatView sessionId="sess-1" sessionStatus="completed" />);
    // Should not throw
    await waitFor(() => {
      expect(screen.getByTestId('chat-view')).toBeInTheDocument();
    });
  });
});
