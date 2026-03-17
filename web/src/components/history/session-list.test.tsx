import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { SessionList } from './session-list';
import * as api from '@/api/client';
import type { Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  deleteSession: vi.fn(),
  downloadSessionOutput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockDeleteSession = vi.mocked(api.deleteSession);
const mockDownloadSessionOutput = vi.mocked(api.downloadSessionOutput);

beforeEach(() => {
  mockDeleteSession.mockReset();
  mockDownloadSessionOutput.mockReset();
});

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    status: 'ready',
    command: 'Fix the bug',
    description: null,
    workdir: '/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('SessionList', () => {
  it('shows empty message when no sessions', () => {
    render(<SessionList sessions={[]} onRefresh={vi.fn()} />);
    expect(screen.getByTestId('empty-message')).toBeInTheDocument();
    expect(screen.getByText('No sessions found.')).toBeInTheDocument();
  });

  it('renders session items', () => {
    const sessions = [makeSession(), makeSession({ id: 'sess-2', name: 'other-task' })];
    render(<SessionList sessions={sessions} onRefresh={vi.fn()} />);
    expect(screen.getByText('my-api')).toBeInTheDocument();
    expect(screen.getByText('other-task')).toBeInTheDocument();
  });

  it('truncates long commands', () => {
    const longCommand = 'A'.repeat(100);
    render(<SessionList sessions={[makeSession({ command: longCommand })]} onRefresh={vi.fn()} />);
    expect(screen.getByText('A'.repeat(80) + '...')).toBeInTheDocument();
  });

  it('does not truncate short commands', () => {
    render(<SessionList sessions={[makeSession({ command: 'Short' })]} onRefresh={vi.fn()} />);
    expect(screen.getByText('Short')).toBeInTheDocument();
  });

  it('expands session details on click', () => {
    render(<SessionList sessions={[makeSession()]} onRefresh={vi.fn()} />);
    expect(screen.queryByTestId('history-detail-sess-1')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    expect(screen.getByTestId('history-detail-sess-1')).toBeInTheDocument();
  });

  it('collapses on second click', () => {
    render(<SessionList sessions={[makeSession()]} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    expect(screen.getByTestId('history-detail-sess-1')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    expect(screen.queryByTestId('history-detail-sess-1')).not.toBeInTheDocument();
  });

  it('expands via keyboard Enter', () => {
    render(<SessionList sessions={[makeSession()]} onRefresh={vi.fn()} />);
    fireEvent.keyDown(screen.getByTestId('history-item-sess-1'), { key: 'Enter' });
    expect(screen.getByTestId('history-detail-sess-1')).toBeInTheDocument();
  });

  it('downloads session log', async () => {
    const blob = new Blob(['log data'], { type: 'text/plain' });
    mockDownloadSessionOutput.mockResolvedValue(blob);
    const revokeUrl = vi.fn();
    vi.stubGlobal('URL', { createObjectURL: () => 'blob://url', revokeObjectURL: revokeUrl });

    render(<SessionList sessions={[makeSession()]} onRefresh={vi.fn()} />);
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    fireEvent.click(screen.getByTestId('download-sess-1'));

    await waitFor(() => {
      expect(mockDownloadSessionOutput).toHaveBeenCalledWith('sess-1');
      expect(revokeUrl).toHaveBeenCalledWith('blob://url');
    });
  });

  it('deletes session and refreshes', async () => {
    mockDeleteSession.mockResolvedValue(undefined);
    const onRefresh = vi.fn();
    render(<SessionList sessions={[makeSession()]} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    fireEvent.click(screen.getByTestId('delete-sess-1'));

    await waitFor(() => {
      expect(mockDeleteSession).toHaveBeenCalledWith('sess-1');
      expect(onRefresh).toHaveBeenCalled();
    });
  });
});
