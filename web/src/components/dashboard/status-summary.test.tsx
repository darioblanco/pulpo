import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatusSummary } from './status-summary';
import type { Session } from '@/api/types';

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    status: 'active',
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
    expect(screen.getByTestId('count-active').textContent).toBe('0');
    expect(screen.getByTestId('count-idle').textContent).toBe('0');
    expect(screen.getByTestId('count-lost').textContent).toBe('0');
    expect(screen.getByTestId('count-finished').textContent).toBe('0');
    expect(screen.getByTestId('count-killed').textContent).toBe('0');
  });

  it('counts sessions by status', () => {
    const sessions = [
      makeSession({ id: '1', status: 'active' }),
      makeSession({ id: '2', status: 'active' }),
      makeSession({ id: '3', status: 'creating' }),
      makeSession({ id: '4', status: 'lost' }),
      makeSession({ id: '5', status: 'finished' }),
      makeSession({ id: '6', status: 'finished' }),
      makeSession({ id: '7', status: 'finished' }),
      makeSession({ id: '8', status: 'killed' }),
      makeSession({ id: '9', status: 'idle' }),
    ];
    render(<StatusSummary sessions={sessions} />);
    expect(screen.getByTestId('count-active').textContent).toBe('3');
    expect(screen.getByTestId('count-idle').textContent).toBe('1');
    expect(screen.getByTestId('count-lost').textContent).toBe('1');
    expect(screen.getByTestId('count-finished').textContent).toBe('3');
    expect(screen.getByTestId('count-killed').textContent).toBe('1');
  });

  it('renders the summary container', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByTestId('status-summary')).toBeInTheDocument();
  });

  it('renders status labels', () => {
    render(<StatusSummary sessions={[]} />);
    expect(screen.getByText('active')).toBeInTheDocument();
    expect(screen.getByText('idle')).toBeInTheDocument();
    expect(screen.getByText('lost')).toBeInTheDocument();
    expect(screen.getByText('done')).toBeInTheDocument();
    expect(screen.getByText('killed')).toBeInTheDocument();
  });
});
