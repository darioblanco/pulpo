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
    expect(screen.getByTestId('status-chip-starting')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-working')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-waiting')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-done')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-lost')).toBeInTheDocument();
  });

  it('has default statuses selected (starting, working, waiting, lost — hides only done)', () => {
    renderFilter();
    expect(screen.getByTestId('status-chip-starting')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-working')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-waiting')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-lost')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-done')).toHaveAttribute('aria-pressed', 'false');
  });

  it('emits filter on search input', () => {
    const { onFilter } = renderFilter();
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'my-api' } });
    expect(onFilter).toHaveBeenCalledWith({
      search: 'my-api',
      statuses: new Set(['starting', 'working', 'waiting', 'lost']),
    });
  });

  it('emits filter with empty search as undefined', () => {
    const { onFilter } = renderFilter();
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: 'test' } });
    fireEvent.change(screen.getByTestId('search-input'), { target: { value: '' } });
    expect(onFilter).toHaveBeenLastCalledWith({
      search: undefined,
      statuses: new Set(['starting', 'working', 'waiting', 'lost']),
    });
  });

  it('toggles status chip off on click (multi-select)', () => {
    const { onFilter } = renderFilter();
    const chip = screen.getByTestId('status-chip-working');
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      statuses: new Set(['starting', 'waiting', 'lost']),
    });
  });

  it('toggles status chip on when clicking unselected chip', () => {
    const { onFilter } = renderFilter();
    const chip = screen.getByTestId('status-chip-done');
    expect(chip).toHaveAttribute('aria-pressed', 'false');
    fireEvent.click(chip);
    expect(chip).toHaveAttribute('aria-pressed', 'true');
    expect(onFilter).toHaveBeenCalledWith({
      search: undefined,
      statuses: new Set(['starting', 'working', 'waiting', 'lost', 'done']),
    });
  });

  it('accepts custom status options and default statuses', () => {
    renderFilter({ statusOptions: ['working'], defaultStatuses: ['working'] });
    expect(screen.getByTestId('status-chip-working')).toBeInTheDocument();
    expect(screen.getByTestId('status-chip-working')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByTestId('status-chip-done')).not.toBeInTheDocument();
  });

  it('accepts empty default statuses', () => {
    renderFilter({ defaultStatuses: [] });
    expect(screen.getByTestId('status-chip-starting')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('status-chip-working')).toHaveAttribute('aria-pressed', 'false');
    expect(screen.getByTestId('status-chip-waiting')).toHaveAttribute('aria-pressed', 'false');
  });

  it('reads initial filter state from URL params', () => {
    const { onFilter } = renderFilter({}, ['/?status=working,done&q=search']);
    expect(screen.getByTestId('search-input')).toHaveValue('search');
    expect(screen.getByTestId('status-chip-working')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-done')).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByTestId('status-chip-waiting')).toHaveAttribute('aria-pressed', 'false');
    // Should have emitted the initial filter from URL
    expect(onFilter).toHaveBeenCalledWith({
      search: 'search',
      statuses: new Set(['working', 'done']),
    });
  });
});
