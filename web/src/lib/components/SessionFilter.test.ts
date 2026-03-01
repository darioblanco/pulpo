import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import SessionFilter from './SessionFilter.svelte';

afterEach(() => {
  cleanup();
});

describe('SessionFilter', () => {
  it('renders searchbar and chip filters', () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    expect(screen.getByPlaceholderText('Search sessions...')).toBeTruthy();
    expect(screen.getByText('completed')).toBeTruthy();
    expect(screen.getByText('dead')).toBeTruthy();
    expect(screen.getByText('claude')).toBeTruthy();
    expect(screen.getByText('codex')).toBeTruthy();
  });

  it('renders custom status and provider options', () => {
    const onfilter = vi.fn();
    render(SessionFilter, {
      props: { onfilter, statusOptions: ['running', 'stale'], providerOptions: ['claude'] },
    });

    expect(screen.getByText('running')).toBeTruthy();
    expect(screen.getByText('stale')).toBeTruthy();
    expect(screen.queryByText('completed')).toBeNull();
    expect(screen.queryByText('codex')).toBeNull();
  });

  it('calls onfilter with search text on input', async () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    const input = screen.getByPlaceholderText('Search sessions...');
    await fireEvent.input(input, { target: { value: 'my-search' } });

    expect(onfilter).toHaveBeenCalledWith(expect.objectContaining({ search: 'my-search' }));
  });

  it('toggles status chip and calls onfilter', async () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    const chip = screen.getByText('completed');
    await fireEvent.click(chip);

    expect(onfilter).toHaveBeenCalledWith(expect.objectContaining({ status: 'completed' }));

    // Click again to deselect
    await fireEvent.click(chip);

    expect(onfilter).toHaveBeenLastCalledWith(expect.objectContaining({ status: undefined }));
  });

  it('toggles provider chip and calls onfilter', async () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    const chip = screen.getByText('claude');
    await fireEvent.click(chip);

    expect(onfilter).toHaveBeenCalledWith(expect.objectContaining({ provider: 'claude' }));
  });

  it('calls onfilter on clear', async () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    const input = screen.getByPlaceholderText('Search sessions...');
    await fireEvent.input(input, { target: { value: 'test' } });
    onfilter.mockClear();

    // Simulate clear by setting value to empty
    await fireEvent.input(input, { target: { value: '' } });

    expect(onfilter).toHaveBeenCalledWith(expect.objectContaining({ search: undefined }));
  });

  it('calls onfilter when clear button is clicked', async () => {
    const onfilter = vi.fn();
    render(SessionFilter, { props: { onfilter } });

    const input = screen.getByPlaceholderText('Search sessions...');
    await fireEvent.input(input, { target: { value: 'test' } });
    onfilter.mockClear();

    // The Konsta Searchbar renders a clear button when value is set
    const clearButtons = document.querySelectorAll('button[type="button"]');
    if (clearButtons.length > 0) {
      await fireEvent.click(clearButtons[clearButtons.length - 1]);
    }

    expect(onfilter).toHaveBeenCalledWith(expect.objectContaining({ search: undefined }));
  });
});
