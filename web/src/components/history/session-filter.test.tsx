import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { SessionFilter } from './session-filter';

describe('SessionFilter', () => {
  it('renders search input and filter chips', () => {
    render(<SessionFilter onFilter={vi.fn()} />);
    expect(screen.getByTestId('search-input')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-finished')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-killed')).toBeInTheDocument();
    expect(screen.getByTestId('provider-chip-claude')).toBeInTheDocument();
    expect(screen.getByTestId('provider-chip-codex')).toBeInTheDocument();
  });

  it('emits filter on search input', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'my-api' } });
    expect(onFilter).toHaveBeenCalledWith({
      search: 'my-api',
      status: undefined,
      provider: undefined,
    });
  });

  it('emits filter with empty search as undefined', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    // Type something first, then clear to empty
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'test' } });
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: '' } });
    expect(onFilter).toHaveBeenLastCalledWith({
      search: undefined,
      status: undefined,
      provider: undefined,
    });
  });

  it('toggles status chip on click', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    const chip = screen.getByTestId('status-chip-finished');
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      status: 'finished',
      provider: undefined,
    });
  });

  it('deactivates status chip on second click', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    const chip = screen.getByTestId('status-chip-finished');
    fireEvent.click(chip);
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(onFilter).toHaveBeenLastCalledWith({
      search: undefined,
      status: undefined,
      provider: undefined,
    });
  });

  it('toggles provider chip on click', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    const chip = screen.getByTestId('provider-chip-claude');
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      status: undefined,
      provider: 'claude',
    });
  });

  it('deactivates provider chip on second click', () => {
    const onFilter = vi.fn();
    render(<SessionFilter onFilter={onFilter} />);
    const chip = screen.getByTestId('provider-chip-claude');
    fireEvent.click(chip);
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(onFilter).toHaveBeenLastCalledWith({
      search: undefined,
      status: undefined,
      provider: undefined,
    });
  });

  it('accepts custom status and provider options', () => {
    render(
      <SessionFilter onFilter={vi.fn()} statusOptions={['active']} providerOptions={['codex']} />,
    );
    expect(screen.getByTestId('status-chip-active')).toBeInTheDocument();
    expect(screen.getByTestId('provider-chip-codex')).toBeInTheDocument();
    expect(screen.queryByTestId('status-chip-finished')).not.toBeInTheDocument();
  });
});
