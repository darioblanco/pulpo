import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { toast } from 'sonner';
import { ScheduleDialog } from './schedule-dialog';
import * as api from '@/api/client';
import type { ScheduleInfo } from '@/api/types';

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock('@/api/client', () => ({
  createSchedule: vi.fn(),
  updateSchedule: vi.fn(),
  getInks: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockCreateSchedule = vi.mocked(api.createSchedule);
const mockUpdateSchedule = vi.mocked(api.updateSchedule);
const mockGetInks = vi.mocked(api.getInks);

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

beforeEach(() => {
  mockCreateSchedule.mockReset();
  mockUpdateSchedule.mockReset();
  mockGetInks.mockReset();
  mockGetInks.mockResolvedValue({ inks: {} });
  vi.mocked(toast.error).mockReset();
  vi.mocked(toast.success).mockReset();
});

function renderDialog(props: { schedule?: ScheduleInfo | null; open?: boolean } = {}) {
  const onOpenChange = vi.fn();
  const onSaved = vi.fn();
  return {
    onOpenChange,
    onSaved,
    ...render(
      <ScheduleDialog
        open={props.open ?? true}
        onOpenChange={onOpenChange}
        schedule={props.schedule ?? null}
        onSaved={onSaved}
      />,
    ),
  };
}

describe('ScheduleDialog', () => {
  it('shows "New Schedule" title for create mode', () => {
    renderDialog();
    expect(screen.getByText('New Schedule')).toBeInTheDocument();
  });

  it('shows "Edit Schedule" title for edit mode', () => {
    renderDialog({ schedule: makeSchedule() });
    expect(screen.getByText('Edit Schedule')).toBeInTheDocument();
  });

  it('pre-populates form fields when editing', () => {
    renderDialog({
      schedule: makeSchedule({
        name: 'my-sched',
        cron: '0 3 * * *',
        command: 'test cmd',
        workdir: '/my/repo',
        target_node: 'remote',
        description: 'My description',
      }),
    });
    expect(screen.getByTestId('schedule-name-input')).toHaveValue('my-sched');
    expect(screen.getByTestId('cron-expr-input')).toHaveValue('0 3 * * *');
    expect(screen.getByTestId('schedule-command-input')).toHaveValue('test cmd');
    expect(screen.getByTestId('schedule-workdir-input')).toHaveValue('/my/repo');
    expect(screen.getByTestId('schedule-target-node-input')).toHaveValue('remote');
    expect(screen.getByTestId('schedule-description-input')).toHaveValue('My description');
  });

  it('resets form fields when creating', () => {
    renderDialog();
    expect(screen.getByTestId('schedule-name-input')).toHaveValue('');
    expect(screen.getByTestId('cron-expr-input')).toHaveValue('');
    expect(screen.getByTestId('schedule-command-input')).toHaveValue('');
  });

  // Validation

  it('shows error when name is empty', async () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent('Name is required');
    });
  });

  it('shows error when cron is empty', async () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'my-sched' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent(
        'Cron expression is required',
      );
    });
  });

  it('shows error when cron is invalid', async () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'my-sched' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: 'not valid' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent(
        'Invalid cron expression',
      );
    });
  });

  it('shows error when neither command nor ink is provided', async () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'my-sched' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent(
        'Command or ink is required',
      );
    });
  });

  // Successful create

  it('creates schedule successfully', async () => {
    mockCreateSchedule.mockResolvedValue(makeSchedule());
    const { onOpenChange, onSaved } = renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'my-sched' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'claude code' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(mockCreateSchedule).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'my-sched',
          cron: '0 3 * * *',
          command: 'claude code',
          workdir: '.',
        }),
      );
      expect(toast.success).toHaveBeenCalledWith('Created schedule "my-sched"');
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onSaved).toHaveBeenCalled();
    });
  });

  // Successful edit

  it('updates schedule successfully', async () => {
    const schedule = makeSchedule();
    mockUpdateSchedule.mockResolvedValue({ ...schedule, command: 'updated' });
    const { onOpenChange, onSaved } = renderDialog({ schedule });

    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'updated' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(mockUpdateSchedule).toHaveBeenCalledWith(
        'sched-1',
        expect.objectContaining({ name: 'nightly-review' }),
      );
      expect(toast.success).toHaveBeenCalledWith('Updated schedule "nightly-review"');
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onSaved).toHaveBeenCalled();
    });
  });

  // Error handling

  it('shows error on create failure', async () => {
    mockCreateSchedule.mockRejectedValue(new Error('Duplicate name'));
    renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'dup' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent('Duplicate name');
    });
  });

  it('shows generic error for non-Error failure', async () => {
    mockCreateSchedule.mockRejectedValue('string error');
    renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'test' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-form-error')).toHaveTextContent(
        'Failed to save schedule',
      );
    });
  });

  // Cron description

  it('shows cron hint when expression is empty', () => {
    renderDialog();
    expect(screen.getByTestId('cron-hint')).toHaveTextContent('minute hour day month weekday');
  });

  it('shows cron description for valid expression', () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    expect(screen.getByTestId('cron-description')).toHaveTextContent('Every day at 3:00 AM');
    expect(screen.queryByTestId('cron-hint')).not.toBeInTheDocument();
    expect(screen.queryByTestId('cron-invalid')).not.toBeInTheDocument();
  });

  it('shows error for invalid cron expression', () => {
    renderDialog();
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: 'invalid' },
    });
    expect(screen.getByTestId('cron-invalid')).toHaveTextContent('Invalid cron expression');
    expect(screen.queryByTestId('cron-description')).not.toBeInTheDocument();
    expect(screen.queryByTestId('cron-hint')).not.toBeInTheDocument();
  });

  // Preset selection

  it('sets cron expression when preset is selected and resets to custom on manual edit', () => {
    renderDialog();
    // Initially empty
    expect(screen.getByTestId('cron-expr-input')).toHaveValue('');

    // Manually type a custom cron (this sets preset to "custom")
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '*/30 * * * *' },
    });

    expect(screen.getByTestId('cron-expr-input')).toHaveValue('*/30 * * * *');
  });

  // Preset detection for edit mode

  it('detects matching preset when editing', () => {
    renderDialog({ schedule: makeSchedule({ cron: '0 * * * *' }) });
    // The cron 0 * * * * matches "Every hour" preset
    expect(screen.getByTestId('cron-expr-input')).toHaveValue('0 * * * *');
  });

  // Submit button states

  it('shows "Create Schedule" button text for new schedule', () => {
    renderDialog();
    expect(screen.getByTestId('schedule-submit-button')).toHaveTextContent('Create Schedule');
  });

  it('shows "Update Schedule" button text for edit', () => {
    renderDialog({ schedule: makeSchedule() });
    expect(screen.getByTestId('schedule-submit-button')).toHaveTextContent('Update Schedule');
  });

  it('shows "Saving..." during submission', async () => {
    mockCreateSchedule.mockReturnValue(new Promise(() => {})); // never resolves
    renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'test' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(screen.getByTestId('schedule-submit-button')).toHaveTextContent('Saving...');
    });
  });

  // Ink integration

  it('shows ink selector when inks are available', async () => {
    mockGetInks.mockResolvedValue({
      inks: {
        reviewer: { description: 'Code review', command: 'claude code' },
      },
    });
    renderDialog();
    await waitFor(() => {
      expect(screen.getByTestId('schedule-ink-select')).toBeInTheDocument();
    });
  });

  it('does not show ink selector when no inks', async () => {
    mockGetInks.mockResolvedValue({ inks: {} });
    renderDialog();
    await waitFor(() => {
      expect(mockGetInks).toHaveBeenCalled();
    });
    expect(screen.queryByTestId('schedule-ink-select')).not.toBeInTheDocument();
  });

  // Closed dialog renders nothing
  it('does not render form when closed', () => {
    renderDialog({ open: false });
    expect(screen.queryByTestId('schedule-form')).not.toBeInTheDocument();
  });

  // Optional fields

  it('sends optional fields when filled', async () => {
    mockCreateSchedule.mockResolvedValue(makeSchedule());
    renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'full-test' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test cmd' },
    });
    fireEvent.change(screen.getByTestId('schedule-workdir-input'), {
      target: { value: '/my/repo' },
    });
    fireEvent.change(screen.getByTestId('schedule-target-node-input'), {
      target: { value: 'remote-node' },
    });
    fireEvent.change(screen.getByTestId('schedule-description-input'), {
      target: { value: 'My schedule' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      expect(mockCreateSchedule).toHaveBeenCalledWith({
        name: 'full-test',
        cron: '0 3 * * *',
        command: 'test cmd',
        workdir: '/my/repo',
        target_node: 'remote-node',
        description: 'My schedule',
      });
    });
  });

  it('omits optional empty fields', async () => {
    mockCreateSchedule.mockResolvedValue(makeSchedule());
    renderDialog();

    fireEvent.change(screen.getByTestId('schedule-name-input'), {
      target: { value: 'minimal' },
    });
    fireEvent.change(screen.getByTestId('cron-expr-input'), {
      target: { value: '0 3 * * *' },
    });
    fireEvent.change(screen.getByTestId('schedule-command-input'), {
      target: { value: 'test' },
    });

    fireEvent.submit(screen.getByTestId('schedule-form'));

    await waitFor(() => {
      const call = mockCreateSchedule.mock.calls[0][0];
      expect(call).not.toHaveProperty('target_node');
      expect(call).not.toHaveProperty('description');
      expect(call).not.toHaveProperty('ink');
    });
  });
});
