import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { WatchdogSettings } from './watchdog-settings';

const defaults = {
  enabled: true,
  onEnabledChange: vi.fn(),
  checkIntervalSecs: 30,
  onCheckIntervalSecsChange: vi.fn(),
  idleTimeoutSecs: 300,
  onIdleTimeoutSecsChange: vi.fn(),
  idleAction: 'pause',
  onIdleActionChange: vi.fn(),
};

describe('WatchdogSettings', () => {
  it('renders all fields', () => {
    render(<WatchdogSettings {...defaults} />);
    expect(screen.getByTestId('watchdog-settings')).toBeInTheDocument();
    expect(screen.getByLabelText('Check interval (seconds)')).toHaveValue(30);
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

  it('handles invalid number input with 0', () => {
    const onCheckIntervalSecsChange = vi.fn();
    render(
      <WatchdogSettings {...defaults} onCheckIntervalSecsChange={onCheckIntervalSecsChange} />,
    );
    fireEvent.change(screen.getByLabelText('Check interval (seconds)'), {
      target: { value: '' },
    });
    expect(onCheckIntervalSecsChange).toHaveBeenCalledWith(0);
  });
});
