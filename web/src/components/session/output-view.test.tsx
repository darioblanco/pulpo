import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { OutputView } from './output-view';
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

describe('OutputView', () => {
  it('renders output container', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    expect(screen.getByTestId('output-view')).toBeInTheDocument();
  });

  it('fetches and displays output', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: 'Hello from agent' });
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    await waitFor(() => {
      expect(screen.getByText('Hello from agent')).toBeInTheDocument();
    });
  });

  it('strips ANSI codes from output', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '\x1B[32mGreen text\x1B[0m' });
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    await waitFor(() => {
      expect(screen.getByText('Green text')).toBeInTheDocument();
    });
  });

  it('shows "No output yet" when output is empty', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    expect(screen.getByText('No output yet')).toBeInTheDocument();
  });

  it('shows input field for active sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="active" />);
    expect(screen.getByTestId('output-input')).toBeInTheDocument();
    expect(screen.getByText('Send')).toBeInTheDocument();
  });

  it('shows input field for lost sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="lost" />);
    expect(screen.getByTestId('output-input')).toBeInTheDocument();
    expect(screen.getByText('Send')).toBeInTheDocument();
  });

  it('hides input field for ready sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    expect(screen.queryByTestId('output-input')).not.toBeInTheDocument();
  });

  it('hides input field for killed sessions', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="killed" />);
    expect(screen.queryByTestId('output-input')).not.toBeInTheDocument();
  });

  it('sends input on button click', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    mockSendInput.mockResolvedValue(undefined);
    render(<OutputView sessionId="sess-1" sessionStatus="active" />);

    const input = screen.getByTestId('output-input');
    fireEvent.change(input, { target: { value: 'Hello' } });
    fireEvent.click(screen.getByText('Send'));

    await waitFor(() => {
      expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'Hello\n');
    });
  });

  it('sends input on Enter key', async () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    mockSendInput.mockResolvedValue(undefined);
    render(<OutputView sessionId="sess-1" sessionStatus="active" />);

    const input = screen.getByTestId('output-input');
    fireEvent.change(input, { target: { value: 'Test' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(mockSendInput).toHaveBeenCalledWith('sess-1', 'Test\n');
    });
  });

  it('does not send empty input', () => {
    mockGetSessionOutput.mockResolvedValue({ output: '' });
    render(<OutputView sessionId="sess-1" sessionStatus="active" />);

    fireEvent.click(screen.getByText('Send'));
    expect(mockSendInput).not.toHaveBeenCalled();
  });

  it('updates output on subsequent fetches', async () => {
    let callCount = 0;
    mockGetSessionOutput.mockImplementation(async () => {
      callCount++;
      if (callCount === 1) return { output: 'Part 1' };
      return { output: 'Part 1Part 2' };
    });

    render(<OutputView sessionId="sess-1" sessionStatus="active" />);

    await waitFor(() => {
      expect(screen.getByText('Part 1')).toBeInTheDocument();
    });

    await waitFor(
      () => {
        expect(screen.getByText('Part 1Part 2')).toBeInTheDocument();
      },
      { timeout: 4000 },
    );
  });

  it('polls output for lost sessions', async () => {
    let callCount = 0;
    mockGetSessionOutput.mockImplementation(async () => {
      callCount++;
      if (callCount === 1) return { output: 'Initial' };
      return { output: 'Updated' };
    });

    render(<OutputView sessionId="sess-1" sessionStatus="lost" />);

    await waitFor(() => {
      expect(screen.getByText('Initial')).toBeInTheDocument();
    });

    await waitFor(
      () => {
        expect(screen.getByText('Updated')).toBeInTheDocument();
      },
      { timeout: 4000 },
    );
  });

  it('handles fetch errors silently', async () => {
    mockGetSessionOutput.mockRejectedValue(new Error('Network error'));
    render(<OutputView sessionId="sess-1" sessionStatus="ready" />);
    await waitFor(() => {
      expect(screen.getByTestId('output-view')).toBeInTheDocument();
    });
  });
});
