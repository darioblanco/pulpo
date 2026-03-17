import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { WatchdogSettings } from './watchdog-settings';

const defaults = {
  enabled: true,
  onEnabledChange: vi.fn(),
  memoryThreshold: 85,
  onMemoryThresholdChange: vi.fn(),
  checkIntervalSecs: 30,
  onCheckIntervalSecsChange: vi.fn(),
  breachCount: 3,
  onBreachCountChange: vi.fn(),
  idleTimeoutSecs: 300,
  onIdleTimeoutSecsChange: vi.fn(),
  idleAction: 'pause',
  onIdleActionChange: vi.fn(),
  adoptTmux: true,
  onAdoptTmuxChange: vi.fn(),
};

describe('WatchdogSettings', () => {
  it('renders all fields', () => {
    render(<WatchdogSettings {...defaults} />);
    expect(screen.getByTestId('watchdog-settings')).toBeInTheDocument();
    expect(screen.getByLabelText('Memory threshold (%)')).toHaveValue(85);
    expect(screen.getByLabelText('Check interval (seconds)')).toHaveValue(30);
    expect(screen.getByLabelText('Breach count')).toHaveValue(3);
    expect(screen.getByLabelText('Idle timeout (seconds)')).toHaveValue(300);
  });

  it('shows enabled label when on', () => {
    render(<WatchdogSettings {...defaults} />);
    expect(screen.getByText('Enabled')).toBeInTheDocument();
  });

  it('shows disabled label when off', () => {
    render(<WatchdogSettings {...defaults} enabled={false} />);
    expect(screen.getByText('Disabled')).toBeInTheDocument();
  });

  it('calls onEnabledChange when switch toggled', () => {
    const onEnabledChange = vi.fn();
    render(<WatchdogSettings {...defaults} onEnabledChange={onEnabledChange} />);
    fireEvent.click(screen.getByTestId('watchdog-toggle'));
    expect(onEnabledChange).toHaveBeenCalledWith(false);
  });

  it('calls onMemoryThresholdChange', () => {
    const onMemoryThresholdChange = vi.fn();
    render(<WatchdogSettings {...defaults} onMemoryThresholdChange={onMemoryThresholdChange} />);
    fireEvent.change(screen.getByLabelText('Memory threshold (%)'), {
      target: { value: '90' },
    });
    expect(onMemoryThresholdChange).toHaveBeenCalledWith(90);
  });

  it('calls onCheckIntervalSecsChange', () => {
    const onCheckIntervalSecsChange = vi.fn();
    render(
      <WatchdogSettings {...defaults} onCheckIntervalSecsChange={onCheckIntervalSecsChange} />,
    );
    fireEvent.change(screen.getByLabelText('Check interval (seconds)'), {
      target: { value: '60' },
    });
    expect(onCheckIntervalSecsChange).toHaveBeenCalledWith(60);
  });

  it('calls onBreachCountChange', () => {
    const onBreachCountChange = vi.fn();
    render(<WatchdogSettings {...defaults} onBreachCountChange={onBreachCountChange} />);
    fireEvent.change(screen.getByLabelText('Breach count'), { target: { value: '5' } });
    expect(onBreachCountChange).toHaveBeenCalledWith(5);
  });

  it('calls onIdleTimeoutSecsChange', () => {
    const onIdleTimeoutSecsChange = vi.fn();
    render(<WatchdogSettings {...defaults} onIdleTimeoutSecsChange={onIdleTimeoutSecsChange} />);
    fireEvent.change(screen.getByLabelText('Idle timeout (seconds)'), {
      target: { value: '600' },
    });
    expect(onIdleTimeoutSecsChange).toHaveBeenCalledWith(600);
  });

  it('renders idle action buttons', () => {
    render(<WatchdogSettings {...defaults} />);
    expect(screen.getByTestId('idle-action-pause')).toBeInTheDocument();
    expect(screen.getByTestId('idle-action-kill')).toBeInTheDocument();
  });

  it('highlights active idle action', () => {
    render(<WatchdogSettings {...defaults} idleAction="kill" />);
    expect(screen.getByTestId('idle-action-kill')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('idle-action-pause')).toHaveAttribute('aria-pressed', 'false');
  });

  it('calls onIdleActionChange', () => {
    const onIdleActionChange = vi.fn();
    render(<WatchdogSettings {...defaults} onIdleActionChange={onIdleActionChange} />);
    fireEvent.click(screen.getByTestId('idle-action-kill'));
    expect(onIdleActionChange).toHaveBeenCalledWith('kill');
  });

  it('renders adopt-tmux toggle', () => {
    render(<WatchdogSettings {...defaults} />);
    expect(screen.getByTestId('adopt-tmux-toggle')).toBeInTheDocument();
    expect(screen.getByText('Auto-adopt tmux sessions')).toBeInTheDocument();
  });

  it('calls onAdoptTmuxChange when toggled', () => {
    const onAdoptTmuxChange = vi.fn();
    render(<WatchdogSettings {...defaults} onAdoptTmuxChange={onAdoptTmuxChange} />);
    fireEvent.click(screen.getByTestId('adopt-tmux-toggle'));
    expect(onAdoptTmuxChange).toHaveBeenCalledWith(false);
  });

  it('handles invalid number input with 0', () => {
    const onMemoryThresholdChange = vi.fn();
    render(<WatchdogSettings {...defaults} onMemoryThresholdChange={onMemoryThresholdChange} />);
    fireEvent.change(screen.getByLabelText('Memory threshold (%)'), {
      target: { value: '' },
    });
    expect(onMemoryThresholdChange).toHaveBeenCalledWith(0);
  });
});
