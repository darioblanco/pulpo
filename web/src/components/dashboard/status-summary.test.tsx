import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatusSummary } from './status-summary';
import type { Session } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    status: 'working',
    status_reason: null,
    command: 'Fix',
    description: null,
    workdir: '/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('StatusSummary', () => {
  it('renders zero counts for empty sessions', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByTestId('count-starting').textContent).toBe('0');
    expect(screen.getByTestId('count-working').textContent).toBe('0');
    expect(screen.getByTestId('count-waiting').textContent).toBe('0');
    expect(screen.getByTestId('count-done').textContent).toBe('0');
    expect(screen.getByTestId('count-lost').textContent).toBe('0');
  });

  it('counts sessions by status', () => {
    const sessions = [
      makeSession({ id: '1', status: 'working' }),
      makeSession({ id: '2', status: 'working' }),
      makeSession({ id: '3', status: 'starting' }),
      makeSession({ id: '4', status: 'lost' }),
      makeSession({ id: '5', status: 'done', status_reason: 'exited' }),
      makeSession({ id: '6', status: 'done', status_reason: 'exited' }),
      makeSession({ id: '7', status: 'done', status_reason: 'exited' }),
      makeSession({ id: '8', status: 'done', status_reason: 'stopped' }),
      makeSession({ id: '9', status: 'waiting', status_reason: 'idle' }),
    ];
    render(<StatusSummary sessions={sessions} />);
    expect(screen.getByTestId('count-starting').textContent).toBe('1');
    expect(screen.getByTestId('count-working').textContent).toBe('2');
    expect(screen.getByTestId('count-waiting').textContent).toBe('1');
    expect(screen.getByTestId('count-lost').textContent).toBe('1');
    expect(screen.getByTestId('count-done').textContent).toBe('4');
  });

  it('renders the summary container', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByTestId('status-summary')).toBeInTheDocument();
  });

  it('renders status labels', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByText('starting')).toBeInTheDocument();
    expect(screen.getByText('working')).toBeInTheDocument();
    expect(screen.getByText('waiting')).toBeInTheDocument();
    expect(screen.getByText('lost')).toBeInTheDocument();
    expect(screen.getByText('done')).toBeInTheDocument();
  });
});
