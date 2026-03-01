import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import Page from './+page.svelte';

const mockGetSessions = vi.fn();
const mockDeleteSession = vi.fn();
const mockDownloadSessionOutput = vi.fn();

vi.mock('$lib/api', () => ({
  getSessions: (...args: unknown[]) => mockGetSessions(...args),
  deleteSession: (...args: unknown[]) => mockDeleteSession(...args),
  downloadSessionOutput: (...args: unknown[]) => mockDownloadSessionOutput(...args),
}));

beforeEach(() => {
  mockGetSessions.mockReset();
  mockDeleteSession.mockReset();
  mockDownloadSessionOutput.mockReset();
});

afterEach(() => {
  cleanup();
});

const sampleSessions = [
  {
    id: '1',
    name: 'fix-bug',
    provider: 'claude',
    status: 'completed',
    prompt: 'Fix the authentication bug',
    mode: 'autonomous',
    workdir: '/home/user/repo',
    guard_config: null,
    intervention_reason: null,
    intervention_at: null,
    recovery_count: 0,
    last_output_at: null,
    created_at: '2026-02-18T10:00:00Z',
  },
  {
    id: '2',
    name: 'add-feature',
    provider: 'codex',
    status: 'dead',
    prompt: 'Add user profiles',
    mode: 'interactive',
    workdir: '/home/user/other',
    guard_config: null,
    intervention_reason: null,
    intervention_at: null,
    recovery_count: 0,
    last_output_at: null,
    created_at: '2026-02-17T08:00:00Z',
  },
];

describe('history page', () => {
  it('shows loading state', () => {
    mockGetSessions.mockReturnValue(new Promise(() => {}));
    render(Page);

    expect(screen.getByText('History')).toBeTruthy();
  });

  it('loads and displays sessions', async () => {
    mockGetSessions.mockResolvedValue(sampleSessions);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
      expect(screen.getByText('add-feature')).toBeTruthy();
    });

    expect(mockGetSessions).toHaveBeenCalledWith(
      expect.objectContaining({ status: 'completed,dead' }),
    );
  });

  it('shows error when fetch fails', async () => {
    mockGetSessions.mockRejectedValue(new Error('network error'));
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('Failed to load sessions')).toBeTruthy();
    });
  });

  it('shows empty message when no sessions', async () => {
    mockGetSessions.mockResolvedValue([]);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('No sessions found.')).toBeTruthy();
    });
  });

  it('expands session details on click', async () => {
    mockGetSessions.mockResolvedValue(sampleSessions);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    const item = screen.getByText('fix-bug');
    await fireEvent.click(item);

    await waitFor(() => {
      expect(screen.getByText('Download Log')).toBeTruthy();
      expect(screen.getByText('Delete')).toBeTruthy();
    });
  });

  it('collapses expanded session on second click', async () => {
    mockGetSessions.mockResolvedValue(sampleSessions);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    const item = screen.getByText('fix-bug');
    await fireEvent.click(item);

    await waitFor(() => {
      expect(screen.getByText('Download Log')).toBeTruthy();
    });

    await fireEvent.click(item);

    await waitFor(() => {
      expect(screen.queryByText('Download Log')).toBeNull();
    });
  });

  it('deletes a session', async () => {
    mockGetSessions.mockResolvedValue([...sampleSessions]);
    mockDeleteSession.mockResolvedValue(undefined);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    // Expand first session
    await fireEvent.click(screen.getByText('fix-bug'));

    await waitFor(() => {
      expect(screen.getByText('Delete')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Delete'));

    expect(mockDeleteSession).toHaveBeenCalledWith('1');
    await waitFor(() => {
      expect(screen.queryByText('fix-bug')).toBeNull();
    });
  });

  it('downloads session output', async () => {
    mockGetSessions.mockResolvedValue(sampleSessions);
    const blob = new Blob(['log content'], { type: 'text/plain' });
    mockDownloadSessionOutput.mockResolvedValue(blob);

    // Mock URL.createObjectURL and URL.revokeObjectURL
    const mockCreateObjectURL = vi.fn().mockReturnValue('blob:test-url');
    const mockRevokeObjectURL = vi.fn();
    vi.stubGlobal('URL', {
      ...URL,
      createObjectURL: mockCreateObjectURL,
      revokeObjectURL: mockRevokeObjectURL,
    });

    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('fix-bug'));

    await waitFor(() => {
      expect(screen.getByText('Download Log')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Download Log'));

    expect(mockDownloadSessionOutput).toHaveBeenCalledWith('1');

    vi.unstubAllGlobals();
  });

  it('applies filter when chip is clicked', async () => {
    mockGetSessions.mockResolvedValue(sampleSessions);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    mockGetSessions.mockClear();
    mockGetSessions.mockResolvedValue([sampleSessions[0]]);

    // Click the 'dead' status chip (first match — the chip, not the session status)
    const deadElements = screen.getAllByText('dead');
    await fireEvent.click(deadElements[0]);

    expect(mockGetSessions).toHaveBeenCalledWith(expect.objectContaining({ status: 'dead' }));
  });

  it('truncates long prompts', async () => {
    const longPrompt = 'A'.repeat(100);
    mockGetSessions.mockResolvedValue([{ ...sampleSessions[0], prompt: longPrompt }]);
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('fix-bug')).toBeTruthy();
    });

    // The subtitle should be truncated with ...
    const truncated = 'A'.repeat(80) + '...';
    expect(screen.getByText(truncated)).toBeTruthy();
  });
});
