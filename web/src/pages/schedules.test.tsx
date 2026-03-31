import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { toast } from 'sonner';
import { SchedulesPage } from './schedules';
import * as api from '@/api/client';
import type { ScheduleInfo, Session } from '@/api/types';

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock('@/api/client', () => ({
  getSchedules: vi.fn(),
  getScheduleRuns: vi.fn(),
  updateSchedule: vi.fn(),
  deleteSchedule: vi.fn(),
  createSchedule: vi.fn(),
  getInks: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/components/layout/app-header', () => ({
  AppHeader: ({ title }: { title: string }) => <div data-testid="mock-app-header">{title}</div>,
}));

vi.mock('@/components/schedules/schedule-dialog', () => ({
  ScheduleDialog: ({
    open,
    onOpenChange,
    schedule,
    onSaved,
  }: {
    open: boolean;
    onOpenChange: (v: boolean) => void;
    schedule: ScheduleInfo | null;
    onSaved: () => void;
  }) =>
    open ? (
      <div data-testid="mock-schedule-dialog">
        <span data-testid="dialog-mode">{schedule ? 'edit' : 'create'}</span>
        {schedule && <span data-testid="dialog-schedule-name">{schedule.name}</span>}
        <button data-testid="dialog-close" onClick={() => onOpenChange(false)}>
          Close
        </button>
        <button data-testid="dialog-save" onClick={onSaved}>
          Save
        </button>
      </div>
    ) : null,
}));

const mockGetSchedules = vi.mocked(api.getSchedules);
const mockGetScheduleRuns = vi.mocked(api.getScheduleRuns);
const mockUpdateSchedule = vi.mocked(api.updateSchedule);
const mockDeleteSchedule = vi.mocked(api.deleteSchedule);

function makeSchedule(overrides: Partial<ScheduleInfo> = {}): ScheduleInfo {
  return {
    id: 'sched-1',
    name: 'nightly-review',
    cron: '0 3 * * *',
    command: 'claude -p "review code"',
    workdir: '/repo',
    target_node: null,
    ink: null,
    description: null,
    enabled: true,
    last_run_at: null,
    last_session_id: null,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

function makeRun(overrides: Partial<Session> = {}): Session {
  return {
    id: 'run-1',
    name: 'nightly-review-001',
    status: 'stopped',
    command: 'claude -p "review code"',
    description: null,
    workdir: '/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    created_at: '2026-03-20T03:00:00Z',
    updated_at: '2026-03-20T03:15:00Z',
    ...overrides,
  };
}

beforeEach(() => {
  mockGetSchedules.mockReset();
  mockGetScheduleRuns.mockReset();
  mockUpdateSchedule.mockReset();
  mockDeleteSchedule.mockReset();
  vi.mocked(toast.error).mockReset();
  vi.mocked(toast.success).mockReset();
});

function renderPage() {
  return render(
    <MemoryRouter>
      <SchedulesPage />
    </MemoryRouter>,
  );
}

describe('SchedulesPage', () => {
  it('shows loading skeleton initially', () => {
    mockGetSchedules.mockReturnValue(new Promise(() => {}));
    renderPage();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows empty state when no schedules exist', async () => {
    mockGetSchedules.mockResolvedValue([]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
      expect(screen.getByText(/No schedules configured yet/)).toBeInTheDocument();
      expect(screen.getByText(/pulpo schedule add/)).toBeInTheDocument();
    });
  });

  it('shows no-match message when filters exclude all schedules', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-table')).toBeInTheDocument();
    });

    // Type a search query that doesn't match
    fireEvent.change(screen.getByTestId('schedule-search-input'), {
      target: { value: 'nonexistent' },
    });

    expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    expect(screen.getByText(/No schedules match your filters/)).toBeInTheDocument();
  });

  it('renders schedule table with data', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'weekly-deploy', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-table')).toBeInTheDocument();
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
      expect(screen.getByTestId('schedule-row-weekly-deploy')).toBeInTheDocument();
    });
  });

  it('shows active/paused status badges', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'paused-sched', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('status-nightly-review')).toHaveTextContent('Active');
      expect(screen.getByTestId('status-paused-sched')).toHaveTextContent('Paused');
    });
  });

  it('shows schedule description when present', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ description: 'Run nightly code review' })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('Run nightly code review')).toBeInTheDocument();
    });
  });

  it('shows ink label when no command', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ command: '', ink: 'reviewer' })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('ink: reviewer')).toBeInTheDocument();
    });
  });

  it('shows (default) when no command or ink', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ command: '' })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('(default)')).toBeInTheDocument();
    });
  });

  // Filtering by name

  it('filters schedules by search query', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'weekly-deploy' }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
      expect(screen.getByTestId('schedule-row-weekly-deploy')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('schedule-search-input'), {
      target: { value: 'nightly' },
    });

    expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    expect(screen.queryByTestId('schedule-row-weekly-deploy')).not.toBeInTheDocument();
  });

  it('filters by search query case-insensitively', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('schedule-search-input'), {
      target: { value: 'NIGHTLY' },
    });

    expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
  });

  // Status filter tabs with counts

  it('shows correct status counts', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'active-2' }),
      makeSchedule({ id: 'sched-3', name: 'paused-sched', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('filter-all')).toHaveTextContent('all(3)');
      expect(screen.getByTestId('filter-active')).toHaveTextContent('active(2)');
      expect(screen.getByTestId('filter-paused')).toHaveTextContent('paused(1)');
    });
  });

  it('filters by active status', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'paused-sched', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-table')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('filter-active'));

    expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    expect(screen.queryByTestId('schedule-row-paused-sched')).not.toBeInTheDocument();
  });

  it('filters by paused status', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'paused-sched', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-table')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('filter-paused'));

    expect(screen.queryByTestId('schedule-row-nightly-review')).not.toBeInTheDocument();
    expect(screen.getByTestId('schedule-row-paused-sched')).toBeInTheDocument();
  });

  it('returns to all when "all" filter is clicked', async () => {
    mockGetSchedules.mockResolvedValue([
      makeSchedule(),
      makeSchedule({ id: 'sched-2', name: 'paused-sched', enabled: false }),
    ]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-table')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('filter-active'));
    expect(screen.queryByTestId('schedule-row-paused-sched')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('filter-all'));
    expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    expect(screen.getByTestId('schedule-row-paused-sched')).toBeInTheDocument();
  });

  // Create dialog

  it('opens create dialog when New Schedule is clicked', async () => {
    mockGetSchedules.mockResolvedValue([]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('new-schedule-button')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('new-schedule-button'));

    expect(screen.getByTestId('mock-schedule-dialog')).toBeInTheDocument();
    expect(screen.getByTestId('dialog-mode')).toHaveTextContent('create');
  });

  // Edit dialog

  it('opens edit dialog when edit button is clicked', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('edit-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('edit-nightly-review'));

    expect(screen.getByTestId('mock-schedule-dialog')).toBeInTheDocument();
    expect(screen.getByTestId('dialog-mode')).toHaveTextContent('edit');
    expect(screen.getByTestId('dialog-schedule-name')).toHaveTextContent('nightly-review');
  });

  // Toggle (pause/resume)

  it('toggles schedule enabled state', async () => {
    const schedule = makeSchedule();
    mockGetSchedules.mockResolvedValue([schedule]);
    mockUpdateSchedule.mockResolvedValue({ ...schedule, enabled: false });
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('toggle-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('toggle-nightly-review'));

    await waitFor(() => {
      expect(mockUpdateSchedule).toHaveBeenCalledWith('sched-1', { enabled: false });
      expect(toast.success).toHaveBeenCalledWith('nightly-review paused');
    });
  });

  it('shows toast on toggle error', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockUpdateSchedule.mockRejectedValue(new Error('Server error'));
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('toggle-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('toggle-nightly-review'));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Server error');
    });
  });

  it('shows generic toast on toggle non-Error failure', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockUpdateSchedule.mockRejectedValue('string error');
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('toggle-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('toggle-nightly-review'));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Failed to update');
    });
  });

  // Delete confirmation

  it('shows delete confirmation dialog', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('delete-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-nightly-review'));

    await waitFor(() => {
      expect(screen.getByText('Delete Schedule')).toBeInTheDocument();
    });
  });

  it('deletes schedule after confirmation', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockDeleteSchedule.mockResolvedValue(undefined);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('delete-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-nightly-review'));

    await waitFor(() => {
      expect(screen.getByText('Delete Schedule')).toBeInTheDocument();
    });

    // Click the Delete action button in the alert dialog footer
    const buttons = screen.getAllByRole('button');
    const deleteActionBtn = buttons.find(
      (btn) => btn.textContent === 'Delete' && btn !== screen.getByTestId('delete-nightly-review'),
    );
    expect(deleteActionBtn).toBeDefined();
    fireEvent.click(deleteActionBtn!);

    await waitFor(() => {
      expect(mockDeleteSchedule).toHaveBeenCalledWith('sched-1');
      expect(toast.success).toHaveBeenCalledWith('Deleted "nightly-review"');
    });
  });

  it('shows toast on delete error', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockDeleteSchedule.mockRejectedValue(new Error('Forbidden'));
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('delete-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-nightly-review'));
    await waitFor(() => {
      expect(screen.getByText('Delete Schedule')).toBeInTheDocument();
    });
    const buttons = screen.getAllByRole('button');
    const deleteActionBtn = buttons.find(
      (btn) => btn.textContent === 'Delete' && btn !== screen.getByTestId('delete-nightly-review'),
    );
    fireEvent.click(deleteActionBtn!);

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Forbidden');
    });
  });

  it('shows generic toast on delete non-Error failure', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockDeleteSchedule.mockRejectedValue('string error');
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('delete-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-nightly-review'));
    await waitFor(() => {
      expect(screen.getByText('Delete Schedule')).toBeInTheDocument();
    });
    const buttons = screen.getAllByRole('button');
    const deleteActionBtn = buttons.find(
      (btn) => btn.textContent === 'Delete' && btn !== screen.getByTestId('delete-nightly-review'),
    );
    fireEvent.click(deleteActionBtn!);

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Failed to delete');
    });
  });

  it('cancels delete dialog', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('delete-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-nightly-review'));
    await waitFor(() => {
      expect(screen.getByText('Cancel')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Cancel'));
    expect(mockDeleteSchedule).not.toHaveBeenCalled();
  });

  // Run history expansion

  it('expands run history on row click', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockResolvedValue([makeRun()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    // Initially shows right chevron
    expect(screen.getByTestId('chevron-right-nightly-review')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));

    // Now shows down chevron
    expect(screen.getByTestId('chevron-down-nightly-review')).toBeInTheDocument();

    await waitFor(() => {
      expect(mockGetScheduleRuns).toHaveBeenCalledWith('sched-1');
      expect(screen.getByTestId('runs-panel-sched-1')).toBeInTheDocument();
      expect(screen.getByTestId('run-run-1')).toBeInTheDocument();
      expect(screen.getByText('nightly-review-001')).toBeInTheDocument();
    });
  });

  it('collapses run history on second row click', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockResolvedValue([makeRun()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    // Expand
    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));
    await waitFor(() => {
      expect(screen.getByTestId('runs-panel-sched-1')).toBeInTheDocument();
    });

    // Collapse
    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));
    expect(screen.queryByTestId('runs-panel-sched-1')).not.toBeInTheDocument();
  });

  it('shows empty runs message', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockResolvedValue([]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));

    await waitFor(() => {
      expect(screen.getByTestId('runs-empty-sched-1')).toBeInTheDocument();
      expect(screen.getByText('No runs yet')).toBeInTheDocument();
    });
  });

  it('shows loading state for run history', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockReturnValue(new Promise(() => {})); // never resolves
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));

    await waitFor(() => {
      expect(screen.getByTestId('runs-loading-sched-1')).toBeInTheDocument();
    });
  });

  it('shows toast on run history fetch error', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockRejectedValue(new Error('Network error'));
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Failed to load run history');
    });
  });

  it('shows last run relative time when present', async () => {
    const recentDate = new Date(Date.now() - 60_000).toISOString();
    mockGetSchedules.mockResolvedValue([makeSchedule({ last_run_at: recentDate })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('1 minute ago')).toBeInTheDocument();
    });
  });

  it('shows "never" when no last run', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ last_run_at: null })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('never')).toBeInTheDocument();
    });
  });

  it('shows "paused" instead of next run for disabled schedules', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ enabled: false })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByText('paused')).toBeInTheDocument();
    });
  });

  it('shows toast on initial load failure', async () => {
    mockGetSchedules.mockRejectedValue(new Error('Network error'));
    renderPage();
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith('Failed to load schedules');
    });
  });

  it('refreshes schedules when dialog saves', async () => {
    mockGetSchedules.mockResolvedValue([]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('new-schedule-button')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('new-schedule-button'));
    expect(screen.getByTestId('mock-schedule-dialog')).toBeInTheDocument();

    // Simulate save
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    fireEvent.click(screen.getByTestId('dialog-save'));

    await waitFor(() => {
      // getSchedules called again after save
      expect(mockGetSchedules).toHaveBeenCalledTimes(2);
    });
  });

  it('shows cron expression for each schedule', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('cron-nightly-review')).toHaveTextContent('0 3 * * *');
    });
  });

  it('shows run with active status without duration end', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule()]);
    mockGetScheduleRuns.mockResolvedValue([makeRun({ status: 'active', updated_at: undefined })]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-row-nightly-review')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('schedule-row-nightly-review'));

    await waitFor(() => {
      expect(screen.getByTestId('runs-panel-sched-1')).toBeInTheDocument();
    });
  });

  it('applies opacity to paused schedule rows', async () => {
    mockGetSchedules.mockResolvedValue([makeSchedule({ enabled: false })]);
    renderPage();
    await waitFor(() => {
      const row = screen.getByTestId('schedule-row-nightly-review');
      expect(row.className).toContain('opacity-50');
    });
  });
});
