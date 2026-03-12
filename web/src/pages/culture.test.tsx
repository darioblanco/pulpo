import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { CulturePage } from './culture';
import * as api from '@/api/client';
import * as sseHook from '@/hooks/use-sse';
import type { Culture } from '@/api/types';

vi.mock('@/api/client', () => ({
  listCulture: vi.fn(),
  deleteCulture: vi.fn(),
  pushCulture: vi.fn(),
  approveCulture: vi.fn(),
  listCultureFiles: vi.fn(),
  readCultureFile: vi.fn(),
  getCultureSyncStatus: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/hooks/use-sse', () => ({
  useSSE: vi.fn().mockReturnValue({ cultureVersion: 0 }),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockListCulture = vi.mocked(api.listCulture);
const mockDeleteCulture = vi.mocked(api.deleteCulture);
const mockPushCulture = vi.mocked(api.pushCulture);
const mockApproveCulture = vi.mocked(api.approveCulture);
const mockListCultureFiles = vi.mocked(api.listCultureFiles);
const mockReadCultureFile = vi.mocked(api.readCultureFile);
const mockGetCultureSyncStatus = vi.mocked(api.getCultureSyncStatus);

beforeEach(() => {
  mockListCulture.mockReset();
  mockDeleteCulture.mockReset();
  mockPushCulture.mockReset();
  mockApproveCulture.mockReset();
  mockListCultureFiles.mockReset();
  mockReadCultureFile.mockReset();
  mockGetCultureSyncStatus.mockReset();
  // Default: sync status disabled
  mockGetCultureSyncStatus.mockResolvedValue({
    enabled: false,
    last_sync: null,
    last_error: null,
    pending_commits: 0,
    total_syncs: 0,
  });
  // Default: file browser returns some files
  mockListCultureFiles.mockResolvedValue({
    files: [
      { path: 'culture', is_dir: true },
      { path: 'culture/AGENTS.md', is_dir: false },
    ],
  });
});

function makeCulture(overrides: Partial<Culture> = {}): Culture {
  return {
    id: 'k-1',
    session_id: 'sess-1',
    kind: 'summary',
    scope_repo: '/tmp/repo',
    scope_ink: 'coder',
    title: 'Auth bug fix',
    body: 'Fixed the authentication token refresh issue',
    tags: ['claude', 'completed'],
    relevance: 0.7,
    created_at: '2025-01-01T00:00:00Z',
    last_referenced_at: null,
    ...overrides,
  };
}

function renderCulture() {
  return render(
    <ConnectionProvider>
      <TooltipProvider>
        <SidebarProvider>
          <CulturePage />
        </SidebarProvider>
      </TooltipProvider>
    </ConnectionProvider>,
  );
}

async function switchToEntriesTab() {
  const user = userEvent.setup();
  await user.click(screen.getByTestId('entries-tab'));
}

describe('CulturePage', () => {
  it('renders with tabs', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    expect(screen.getByTestId('culture-page')).toBeInTheDocument();
    expect(screen.getByTestId('culture-tabs')).toBeInTheDocument();
    expect(screen.getByTestId('files-tab')).toBeInTheDocument();
    expect(screen.getByTestId('entries-tab')).toBeInTheDocument();
  });

  it('shows file browser by default', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-tree')).toBeInTheDocument();
    });
  });

  it('shows empty message in entries tab', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(
        screen.getByText('No culture items yet. Culture is extracted from completed sessions.'),
      ).toBeInTheDocument();
    });
  });

  it('shows items in entries tab after loading', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });
  });

  it('shows error on fetch failure in entries tab', async () => {
    mockListCulture.mockRejectedValue(new Error('Network error'));
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('Failed to load culture')).toBeInTheDocument();
    });
  });

  it('deletes an item', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    mockDeleteCulture.mockResolvedValue({ deleted: true });
    renderCulture();
    await switchToEntriesTab();

    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByTestId('delete-culture-btn'));

    await waitFor(() => {
      expect(mockDeleteCulture).toHaveBeenCalledWith('k-1');
    });
  });

  it('renders push button', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    expect(screen.getByTestId('push-culture-btn')).toBeInTheDocument();
  });

  it('pushes culture to remote', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockPushCulture.mockResolvedValue({ pushed: true, message: 'pushed to remote' });
    renderCulture();

    const user = userEvent.setup();
    await user.click(screen.getByTestId('push-culture-btn'));

    await waitFor(() => {
      expect(mockPushCulture).toHaveBeenCalled();
    });
  });

  it('shows failure badge for failure items', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ kind: 'failure', title: 'OOM crash' })],
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('failure')).toBeInTheDocument();
    });
  });

  it('shows tags on culture cards', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('claude')).toBeInTheDocument();
      expect(screen.getByText('completed')).toBeInTheDocument();
    });
  });

  it('shows item count', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture(), makeCulture({ id: 'k-2', title: 'Second item' })],
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('2 items')).toBeInTheDocument();
    });
  });

  it('shows approve button for stale items', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ tags: ['stale', 'claude'], title: 'Stale finding' })],
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByTestId('approve-culture-btn')).toBeInTheDocument();
    });
  });

  it('does not show approve button for non-stale items', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('approve-culture-btn')).not.toBeInTheDocument();
  });

  it('approves a stale item', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ tags: ['stale', 'claude'], title: 'Stale finding' })],
    });
    mockApproveCulture.mockResolvedValue({
      culture: {
        ...makeCulture({ tags: ['claude'], title: 'Stale finding' }),
        last_referenced_at: '2025-03-12T00:00:00Z',
      },
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      expect(screen.getByTestId('approve-culture-btn')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByTestId('approve-culture-btn'));

    await waitFor(() => {
      expect(mockApproveCulture).toHaveBeenCalledWith('k-1');
    });
  });

  it('shows superseded items with reduced opacity', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ tags: ['superseded'], title: 'Old approach' })],
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      const card = screen.getByTestId('culture-card');
      expect(card).toHaveClass('opacity-60');
    });
  });

  it('shows stale items with reduced opacity', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ tags: ['stale', 'claude'], title: 'Old finding' })],
    });
    renderCulture();
    await switchToEntriesTab();
    await waitFor(() => {
      const card = screen.getByTestId('culture-card');
      expect(card).toHaveClass('opacity-60');
    });
  });
});

describe('CultureFileBrowser', () => {
  it('shows file tree', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-tree')).toBeInTheDocument();
      expect(screen.getAllByTestId('dir-entry').length).toBeGreaterThan(0);
      expect(screen.getAllByTestId('file-entry').length).toBeGreaterThan(0);
    });
  });

  it('opens a file on click', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockReadCultureFile.mockResolvedValue({
      path: 'culture/AGENTS.md',
      content: '# Culture\n\nShared learnings',
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-tree')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByText('AGENTS.md'));

    await waitFor(() => {
      expect(screen.getByTestId('file-viewer')).toBeInTheDocument();
      expect(mockReadCultureFile).toHaveBeenCalledWith('culture/AGENTS.md');
    });
  });

  it('navigates back from file viewer', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockReadCultureFile.mockResolvedValue({
      path: 'culture/AGENTS.md',
      content: '# Culture',
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-tree')).toBeInTheDocument();
    });

    const user = userEvent.setup();
    await user.click(screen.getByText('AGENTS.md'));
    await waitFor(() => {
      expect(screen.getByTestId('file-viewer')).toBeInTheDocument();
    });

    await user.click(screen.getByTestId('back-btn'));
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-tree')).toBeInTheDocument();
    });
  });

  it('shows error when file list fails', async () => {
    mockListCultureFiles.mockRejectedValue(new Error('Network error'));
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-error')).toBeInTheDocument();
    });
  });

  it('shows empty message when no files', async () => {
    mockListCultureFiles.mockResolvedValue({ files: [] });
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('file-browser-empty')).toBeInTheDocument();
    });
  });
});

describe('SyncStatusBadge', () => {
  it('does not show badge when sync disabled', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockGetCultureSyncStatus.mockResolvedValue({
      enabled: false,
      last_sync: null,
      last_error: null,
      pending_commits: 0,
      total_syncs: 0,
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('culture-page')).toBeInTheDocument();
    });
    expect(screen.queryByTestId('sync-status-badge')).not.toBeInTheDocument();
  });

  it('shows sync badge when enabled', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockGetCultureSyncStatus.mockResolvedValue({
      enabled: true,
      last_sync: '2026-03-12T00:00:00Z',
      last_error: null,
      pending_commits: 0,
      total_syncs: 5,
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('sync-status-badge')).toBeInTheDocument();
      expect(screen.getByText('Synced (5)')).toBeInTheDocument();
    });
  });

  it('shows error badge on sync error', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockGetCultureSyncStatus.mockResolvedValue({
      enabled: true,
      last_sync: null,
      last_error: 'fetch failed',
      pending_commits: 0,
      total_syncs: 0,
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('sync-status-badge')).toBeInTheDocument();
      expect(screen.getByText('Sync error')).toBeInTheDocument();
    });
  });

  it('shows sync enabled badge before first sync', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockGetCultureSyncStatus.mockResolvedValue({
      enabled: true,
      last_sync: null,
      last_error: null,
      pending_commits: 0,
      total_syncs: 0,
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByTestId('sync-status-badge')).toBeInTheDocument();
      expect(screen.getByText('Sync enabled')).toBeInTheDocument();
    });
  });
});

describe('CultureSSE', () => {
  it('re-fetches culture when cultureVersion changes', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    const mockUseSSE = vi.mocked(sseHook.useSSE);
    mockUseSSE.mockReturnValue({
      cultureVersion: 0,
      connected: true,
      sessions: [],
      setSessions: vi.fn(),
    });

    const { rerender } = renderCulture();

    await waitFor(() => {
      expect(mockListCulture).toHaveBeenCalledTimes(1);
    });

    // Simulate a culture SSE event by bumping the version
    mockUseSSE.mockReturnValue({
      cultureVersion: 1,
      connected: true,
      sessions: [],
      setSessions: vi.fn(),
    });
    rerender(
      <ConnectionProvider>
        <TooltipProvider>
          <SidebarProvider>
            <CulturePage />
          </SidebarProvider>
        </TooltipProvider>
      </ConnectionProvider>,
    );

    await waitFor(() => {
      // Initial fetch + re-fetch from version change
      expect(mockListCulture).toHaveBeenCalledTimes(2);
    });
  });

  it('refreshes sync status on culture version change', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    const mockUseSSE = vi.mocked(sseHook.useSSE);
    mockUseSSE.mockReturnValue({
      cultureVersion: 0,
      connected: true,
      sessions: [],
      setSessions: vi.fn(),
    });

    const { rerender } = renderCulture();

    await waitFor(() => {
      expect(mockGetCultureSyncStatus).toHaveBeenCalledTimes(1);
    });

    mockUseSSE.mockReturnValue({
      cultureVersion: 1,
      connected: true,
      sessions: [],
      setSessions: vi.fn(),
    });
    rerender(
      <ConnectionProvider>
        <TooltipProvider>
          <SidebarProvider>
            <CulturePage />
          </SidebarProvider>
        </TooltipProvider>
      </ConnectionProvider>,
    );

    await waitFor(() => {
      expect(mockGetCultureSyncStatus).toHaveBeenCalledTimes(2);
    });
  });
});
