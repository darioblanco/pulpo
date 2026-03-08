import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GuardSettings } from './guard-settings';

describe('GuardSettings', () => {
  it('renders the card', () => {
    render(<GuardSettings unrestricted={false} onUnrestrictedChange={vi.fn()} />);
    expect(screen.getByTestId('guard-settings')).toBeInTheDocument();
    expect(screen.getByText('Guards')).toBeInTheDocument();
  });

  it('renders unrestricted toggle unchecked by default', () => {
    render(<GuardSettings unrestricted={false} onUnrestrictedChange={vi.fn()} />);
    const toggle = screen.getByTestId('guard-unrestricted-toggle');
    expect(toggle).toBeInTheDocument();
    expect(toggle).toHaveAttribute('data-state', 'unchecked');
  });

  it('renders unrestricted toggle checked when true', () => {
    render(<GuardSettings unrestricted={true} onUnrestrictedChange={vi.fn()} />);
    const toggle = screen.getByTestId('guard-unrestricted-toggle');
    expect(toggle).toHaveAttribute('data-state', 'checked');
  });

  it('calls onUnrestrictedChange when toggled', () => {
    const onUnrestrictedChange = vi.fn();
    render(<GuardSettings unrestricted={false} onUnrestrictedChange={onUnrestrictedChange} />);
    fireEvent.click(screen.getByTestId('guard-unrestricted-toggle'));
    expect(onUnrestrictedChange).toHaveBeenCalledWith(true);
  });

  it('shows warning text about safety', () => {
    render(<GuardSettings unrestricted={false} onUnrestrictedChange={vi.fn()} />);
    expect(screen.getByText(/without safety guardrails/)).toBeInTheDocument();
  });

  it('has label for unrestricted mode', () => {
    render(<GuardSettings unrestricted={false} onUnrestrictedChange={vi.fn()} />);
    expect(screen.getByLabelText('Unrestricted mode')).toBeInTheDocument();
  });
});
