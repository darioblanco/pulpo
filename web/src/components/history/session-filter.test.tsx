import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { SessionFilter } from './session-filter';

function renderFilter(
  props: {
    onFilter?: (query: { search?: string; statuses: Set<string> }) => void;
    statusOptions?: string[];
    defaultStatuses?: string[];
  } = {},
  initialEntries: string[] = ['/'],
) {
  const onFilter = props.onFilter ?? vi.fn();
  return {
    onFilter,
    ...render(
      <MemoryRouter initialEntries={initialEntries}>
        <SessionFilter
          onFilter={onFilter}
          statusOptions={props.statusOptions}
          defaultStatuses={props.defaultStatuses}
        />
      </MemoryRouter>,
    ),
  };
}

describe('SessionFilter', () => {
  it('renders search input and default filter chips', () => {
    renderFilter();
    expect(screen.getByTestId('search-input')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-active')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-idle')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-ready')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-stopped')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-lost')).toBeInTheDocument();
  });

  it('has default statuses selected (active, idle, ready)', () => {
    renderFilter();
    expect(screen.getByTestId('status-chip-active')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-idle')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-ready')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-stopped')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('status-chip-lost')).toHaveAttribute('aria-pressed', 'false');
  });

  it('emits filter on search input', () => {
    const { onFilter } = renderFilter();
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'my-api' } });
    expect(onFilter).toHaveBeenCalledWith({
      search: 'my-api',
      statuses: new Set(['active', 'idle', 'ready']),
    });
  });

  it('emits filter with empty search as undefined', () => {
    const { onFilter } = renderFilter();
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'test' } });
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: '' } });
    expect(onFilter).toHaveBeenLastCalledWith({
      search: undefined,
      statuses: new Set(['active', 'idle', 'ready']),
    });
  });

  it('toggles status chip off on click (multi-select)', () => {
    const { onFilter } = renderFilter();
    const chip = screen.getByTestId('status-chip-active');
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      statuses: new Set(['idle', 'ready']),
    });
  });

  it('toggles status chip on when clicking unselected chip', () => {
    const { onFilter } = renderFilter();
    const chip = screen.getByTestId('status-chip-stopped');
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      statuses: new Set(['active', 'idle', 'ready', 'stopped']),
    });
  });

  it('accepts custom status options and default statuses', () => {
    renderFilter({ statusOptions: ['active'], defaultStatuses: ['active'] });
    expect(screen.getByTestId('status-chip-active')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-active')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByTestId('status-chip-ready')).not.toBeInTheDocument();
  });

  it('accepts empty default statuses', () => {
    renderFilter({ defaultStatuses: [] });
    expect(screen.getByTestId('status-chip-active')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('status-chip-idle')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('status-chip-ready')).toHaveAttribute('aria-pressed', 'false');
  });

  it('reads initial filter state from URL params', () => {
    const { onFilter } = renderFilter({}, ['/?status=active,stopped&q=search']);
    expect(screen.getByTestId('search-input')).toHaveValue('search');
    expect(screen.getByTestId('status-chip-active')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-stopped')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-idle')).toHaveAttribute('aria-pressed', 'false');
    // Should have emitted the initial filter from URL
    expect(onFilter).toHaveBeenCalledWith({
      search: 'search',
      statuses: new Set(['active', 'stopped']),
    });
  });
});
