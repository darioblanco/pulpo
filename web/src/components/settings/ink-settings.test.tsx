import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { InkSettings } from './ink-settings';
import type { InkConfig } from '@/api/types';

const emptyInk: InkConfig = {
  description: null,
  provider: null,
  model: null,
  mode: null,
  guard_preset: null,
  allowed_tools: null,
  system_prompt: null,
  max_turns: null,
  max_budget_usd: null,
  output_format: null,
};

const reviewerInk: InkConfig = {
  description: 'Code review specialist',
  provider: 'claude',
  model: null,
  mode: 'interactive',
  guard_preset: 'strict',
  allowed_tools: null,
  system_prompt: 'You are a code reviewer.',
  max_turns: 5,
  max_budget_usd: 1.0,
  output_format: null,
};

describe('InkSettings', () => {
  it('renders the card', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-settings')).toBeInTheDocument();
    expect(screen.getByText('Inks')).toBeInTheDocument();
  });

  it('shows empty state when no inks', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-empty')).toBeInTheDocument();
  });

  it('lists ink names sorted alphabetically', () => {
    const inks = {
      coder: { ...emptyInk },
      reviewer: { ...reviewerInk },
      'quick-fix': { ...emptyInk },
    };
    render(<InkSettings inks={inks} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-coder')).toBeInTheDocument();
    expect(screen.getByTestId('ink-quick-fix')).toBeInTheDocument();
    expect(screen.getByTestId('ink-reviewer')).toBeInTheDocument();
  });

  it('shows ink description in collapsed view', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.getByText('Code review specialist')).toBeInTheDocument();
  });

  it('expands ink editor on click', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.queryByTestId('ink-editor-reviewer')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByTestId('ink-editor-reviewer')).toBeInTheDocument();
  });

  it('collapses ink editor on second click', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByTestId('ink-editor-reviewer')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.queryByTestId('ink-editor-reviewer')).not.toBeInTheDocument();
  });

  it('displays ink fields in editor', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    expect(screen.getByLabelText('Description')).toHaveValue('Code review specialist');
    expect(screen.getByLabelText('System prompt')).toHaveValue('You are a code reviewer.');
    expect(screen.getByLabelText('Max turns')).toHaveValue(5);
    expect(screen.getByLabelText('Max budget (USD)')).toHaveValue(1);
  });

  it('calls onInksChange when description is updated', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: 'Updated desc' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, description: 'Updated desc' },
    });
  });

  it('calls onInksChange when system prompt is updated', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('System prompt'), {
      target: { value: 'New prompt' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, system_prompt: 'New prompt' },
    });
  });

  it('clears field to null when emptied', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Description'), {
      target: { value: '' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, description: null },
    });
  });

  it('updates model field', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Model'), {
      target: { value: 'opus' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, model: 'opus' },
    });
  });

  it('updates output format field', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Output format'), {
      target: { value: 'json' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, output_format: 'json' },
    });
  });

  it('removes an ink', () => {
    const onInksChange = vi.fn();
    const inks = { reviewer: reviewerInk, coder: { ...emptyInk } };
    render(<InkSettings inks={inks} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.click(screen.getByTestId('ink-remove-reviewer'));
    expect(onInksChange).toHaveBeenCalledWith({ coder: { ...emptyInk } });
  });

  it('adds a new ink', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'my-ink' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).toHaveBeenCalledWith({
      'my-ink': { ...emptyInk },
    });
  });

  it('normalizes new ink name to kebab-case', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'My Custom Ink' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).toHaveBeenCalledWith({
      'my-custom-ink': { ...emptyInk },
    });
  });

  it('prevents adding ink with duplicate name', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.change(screen.getByTestId('ink-new-name'), {
      target: { value: 'reviewer' },
    });
    fireEvent.click(screen.getByTestId('ink-add-btn'));
    expect(onInksChange).not.toHaveBeenCalled();
  });

  it('add button is disabled when name is empty', () => {
    render(<InkSettings inks={{}} onInksChange={vi.fn()} />);
    expect(screen.getByTestId('ink-add-btn')).toBeDisabled();
  });

  it('adds ink on Enter key', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{}} onInksChange={onInksChange} />);
    const input = screen.getByTestId('ink-new-name');
    fireEvent.change(input, { target: { value: 'test-ink' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onInksChange).toHaveBeenCalledWith({
      'test-ink': { ...emptyInk },
    });
  });

  it('does not show empty state when inks exist', () => {
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={vi.fn()} />);
    expect(screen.queryByTestId('ink-empty')).not.toBeInTheDocument();
  });

  it('updates max_turns as number', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Max turns'), {
      target: { value: '10' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, max_turns: 10 },
    });
  });

  it('clears max_turns to null when emptied', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Max turns'), {
      target: { value: '' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, max_turns: null },
    });
  });

  it('updates max_budget_usd as number', () => {
    const onInksChange = vi.fn();
    render(<InkSettings inks={{ reviewer: reviewerInk }} onInksChange={onInksChange} />);
    fireEvent.click(screen.getByTestId('ink-toggle-reviewer'));
    fireEvent.change(screen.getByLabelText('Max budget (USD)'), {
      target: { value: '5.5' },
    });
    expect(onInksChange).toHaveBeenCalledWith({
      reviewer: { ...reviewerInk, max_budget_usd: 5.5 },
    });
  });
});
