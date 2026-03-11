import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { CulturePage } from './culture';
import * as api from '@/api/client';
import type { Culture } from '@/api/types';

vi.mock('@/api/client', () => ({
  listCulture: vi.fn(),
  deleteCulture: vi.fn(),
  pushCulture: vi.fn(),
  listCultureFiles: vi.fn(),
  readCultureFile: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockListCulture = vi.mocked(api.listCulture);
const mockDeleteCulture = vi.mocked(api.deleteCulture);
const mockPushCulture = vi.mocked(api.pushCulture);
const mockListCultureFiles = vi.mocked(api.listCultureFiles);
const mockReadCultureFile = vi.mocked(api.readCultureFile);

beforeEach(() => {
  mockListCulture.mockReset();
  mockDeleteCulture.mockReset();
  mockPushCulture.mockReset();
  mockListCultureFiles.mockReset();
  mockReadCultureFile.mockReset();
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
