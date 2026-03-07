import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GuardSettings } from './guard-settings';

const defaults = {
  preset: 'standard',
  onPresetChange: vi.fn(),
  maxTurns: '',
  onMaxTurnsChange: vi.fn(),
  maxBudgetUsd: '',
  onMaxBudgetUsdChange: vi.fn(),
  outputFormat: '',
  onOutputFormatChange: vi.fn(),
};

describe('GuardSettings', () => {
  it('renders all presets', () => {
    render(<GuardSettings {...defaults} />);
    expect(screen.getByTestId('guard-settings')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-strict')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-standard')).toBeInTheDocument();
    expect(screen.getByTestId('guard-preset-unrestricted')).toBeInTheDocument();
  });

  it('highlights active preset', () => {
    render(<GuardSettings {...defaults} preset="strict" />);
    const strict = screen.getByTestId('guard-preset-strict');
    expect(strict.getAttribute('data-variant')).toBe('default');
    expect(strict).toHaveAttribute('aria-pressed', 'true');
    const standard = screen.getByTestId('guard-preset-standard');
    expect(standard.getAttribute('data-variant')).toBe('outline');
    expect(standard).toHaveAttribute('aria-pressed', 'false');
  });

  it('calls onPresetChange when clicked', () => {
    const onPresetChange = vi.fn();
    render(<GuardSettings {...defaults} onPresetChange={onPresetChange} />);
    fireEvent.click(screen.getByTestId('guard-preset-strict'));
    expect(onPresetChange).toHaveBeenCalledWith('strict');
  });

  it('renders max turns field', () => {
    render(<GuardSettings {...defaults} maxTurns="50" />);
    expect(screen.getByLabelText('Max turns')).toHaveValue(50);
  });

  it('calls onMaxTurnsChange', () => {
    const onMaxTurnsChange = vi.fn();
    render(<GuardSettings {...defaults} onMaxTurnsChange={onMaxTurnsChange} />);
    fireEvent.change(screen.getByLabelText('Max turns'), { target: { value: '25' } });
    expect(onMaxTurnsChange).toHaveBeenCalledWith('25');
  });

  it('renders max budget field', () => {
    render(<GuardSettings {...defaults} maxBudgetUsd="10.50" />);
    expect(screen.getByLabelText('Max budget (USD)')).toHaveValue(10.5);
  });

  it('calls onMaxBudgetUsdChange', () => {
    const onMaxBudgetUsdChange = vi.fn();
    render(<GuardSettings {...defaults} onMaxBudgetUsdChange={onMaxBudgetUsdChange} />);
    fireEvent.change(screen.getByLabelText('Max budget (USD)'), { target: { value: '5.00' } });
    expect(onMaxBudgetUsdChange).toHaveBeenCalledWith('5.00');
  });

  it('renders output format field', () => {
    render(<GuardSettings {...defaults} outputFormat="json" />);
    expect(screen.getByLabelText('Output format')).toHaveValue('json');
  });

  it('calls onOutputFormatChange', () => {
    const onOutputFormatChange = vi.fn();
    render(<GuardSettings {...defaults} onOutputFormatChange={onOutputFormatChange} />);
    fireEvent.change(screen.getByLabelText('Output format'), { target: { value: 'json' } });
    expect(onOutputFormatChange).toHaveBeenCalledWith('json');
  });
});
