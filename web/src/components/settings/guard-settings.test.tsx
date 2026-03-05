import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GuardSettings } from './guard-settings';

describe('GuardSettings', () => {
  it('renders all presets', () => {
    render(<GuardSettings preset="standard" onPresetChange={vi.fn()} />);
    expect(screen.getByTestId('guard-settings')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-strict')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-standard')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-unrestricted')).toBeInTheDocument();
  });

  it('highlights active preset', () => {
    render(<GuardSettings preset="strict" onPresetChange={vi.fn()} />);
    const strict = screen.getByTestId('guard-preset-strict');
    expect(strict.getAttribute('data-variant')).toBe('default');
    expect(strict).toHaveAttribute('aria-pressed', 'true');
    const standard = screen.getByTestId('guard-preset-standard');
    expect(standard.getAttribute('data-variant')).toBe('outline');
    expect(standard).toHaveAttribute('aria-pressed', 'false');
  });

  it('calls onPresetChange when clicked', () => {
    const onPresetChange = vi.fn();
    render(<GuardSettings preset="standard" onPresetChange={onPresetChange} />);
    fireEvent.click(screen.getByTestId('guard-preset-strict'));
    expect(onPresetChange).toHaveBeenCalledWith('strict');
  });
});
