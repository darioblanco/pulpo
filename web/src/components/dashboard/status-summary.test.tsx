import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatusSummary } from './status-summary';
import type { Session } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix',
    mode: 'interactive',
    workdir: '/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    persona: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('StatusSummary', () => {
  it('renders zero counts for empty sessions', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByTestId('count-running').textContent).toBe('0');
    expect(screen.getByTestId('count-stale').textContent).toBe('0');
    expect(screen.getByTestId('count-completed').textContent).toBe('0');
    expect(screen.getByTestId('count-dead').textContent).toBe('0');
  });

  it('counts sessions by status', () => {
    const sessions = [
      makeSession({ id: '1', status: 'running' }),
      makeSession({ id: '2', status: 'running' }),
      makeSession({ id: '3', status: 'creating' }),
      makeSession({ id: '4', status: 'stale' }),
      makeSession({ id: '5', status: 'completed' }),
      makeSession({ id: '6', status: 'completed' }),
      makeSession({ id: '7', status: 'completed' }),
      makeSession({ id: '8', status: 'dead' }),
    ];
    render(<StatusSummary sessions={sessions} />);
    expect(screen.getByTestId('count-running').textContent).toBe('3');
    expect(screen.getByTestId('count-stale').textContent).toBe('1');
    expect(screen.getByTestId('count-completed').textContent).toBe('3');
    expect(screen.getByTestId('count-dead').textContent).toBe('1');
  });

  it('renders the summary container', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByTestId('status-summary')).toBeInTheDocument();
  });

  it('renders status labels', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('stale')).toBeInTheDocument();
    expect(screen.getByText('done')).toBeInTheDocument();
    expect(screen.getByText('dead')).toBeInTheDocument();
  });
});
