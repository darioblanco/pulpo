import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  cn,
  formatDuration,
  formatMemory,
  formatRelativeTime,
  formatSessionStatus,
  isTerminal,
  needsInputReason,
  sessionStatusColor,
} from './utils';

describe('cn', () => {
  it('merges class names', () => {
    expect(cn('foo', 'bar')).toBe('foo bar');
  });

  it('handles conditional classes', () => {
    const show = false;
    expect(cn('foo', show && 'bar', 'baz')).toBe('foo baz');
  });

  it('merges conflicting tailwind classes', () => {
    expect(cn('p-4', 'p-2')).toBe('p-2');
  });
});

describe('formatDuration', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('formats seconds', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:00:30Z'));
    expect(formatDuration('2025-01-01T00:00:00Z')).toBe('30s');
  });

  it('formats minutes', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:05:00Z'));
    expect(formatDuration('2025-01-01T00:00:00Z')).toBe('5m');
  });

  it('formats hours and minutes', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T02:30:00Z'));
    expect(formatDuration('2025-01-01T00:00:00Z')).toBe('2h 30m');
  });
});

describe('formatRelativeTime', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns just now for recent times', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:00:05Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('just now');
  });

  it('returns seconds ago', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:00:30Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('30 seconds ago');
  });

  it('returns minutes ago (singular)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:01:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('1 minute ago');
  });

  it('returns minutes ago (plural)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T00:05:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('5 minutes ago');
  });

  it('returns hours ago (singular)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T01:00:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('1 hour ago');
  });

  it('returns hours ago (plural)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-01T03:00:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('3 hours ago');
  });

  it('returns days ago (singular)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-02T00:00:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('1 day ago');
  });

  it('returns days ago (plural)', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2025-01-04T00:00:00Z'));
    expect(formatRelativeTime('2025-01-01T00:00:00Z')).toBe('3 days ago');
  });
});

describe('formatMemory', () => {
  it('formats megabytes below 1 GB', () => {
    expect(formatMemory(512)).toBe('512 MB');
  });

  it('formats exactly 1 GB', () => {
    expect(formatMemory(1024)).toBe('1 GB');
  });

  it('formats 16 GB', () => {
    expect(formatMemory(16384)).toBe('16 GB');
  });

  it('formats 64 GB', () => {
    expect(formatMemory(65536)).toBe('64 GB');
  });

  it('rounds to nearest GB', () => {
    expect(formatMemory(4000)).toBe('4 GB');
  });
});

describe('needsInputReason', () => {
  it('extracts the reason from a needs_input: prefix', () => {
    expect(needsInputReason('needs_input:permission')).toBe('permission');
  });

  it('returns undefined for plain idle', () => {
    expect(needsInputReason('idle')).toBeUndefined();
  });

  it('returns undefined for null/undefined', () => {
    expect(needsInputReason(null)).toBeUndefined();
    expect(needsInputReason(undefined)).toBeUndefined();
  });

  it('treats the suffix as opaque text, unmangled', () => {
    expect(needsInputReason('needs_input:Some-Weird_Reason')).toBe('Some-Weird_Reason');
  });
});

describe('formatSessionStatus', () => {
  it('renders bare starting/working/lost with no parenthetical', () => {
    expect(formatSessionStatus({ status: 'starting', status_reason: null })).toBe('starting');
    expect(formatSessionStatus({ status: 'working', status_reason: null })).toBe('working');
    expect(formatSessionStatus({ status: 'lost', status_reason: null })).toBe('lost');
  });

  it('renders waiting (idle) for plain idle', () => {
    expect(formatSessionStatus({ status: 'waiting', status_reason: 'idle' })).toBe(
      'waiting (idle)',
    );
  });

  it('renders waiting (needs input: <reason>) verbatim', () => {
    expect(
      formatSessionStatus({ status: 'waiting', status_reason: 'needs_input:permission' }),
    ).toBe('waiting (needs input: permission)');
    expect(
      formatSessionStatus({ status: 'waiting', status_reason: 'needs_input:Some-Weird_Reason' }),
    ).toBe('waiting (needs input: Some-Weird_Reason)');
  });

  it('renders bare waiting when status_reason is absent', () => {
    expect(formatSessionStatus({ status: 'waiting', status_reason: null })).toBe('waiting');
  });

  it('renders done (exit N) when exited with a numeric exit_code', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'exited', exit_code: 0 })).toBe(
      'done (exit 0)',
    );
    expect(formatSessionStatus({ status: 'done', status_reason: 'exited', exit_code: 1 })).toBe(
      'done (exit 1)',
    );
  });

  it('renders done (exited) when exited with no exit_code', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'exited' })).toBe('done (exited)');
    expect(formatSessionStatus({ status: 'done', status_reason: 'exited', exit_code: null })).toBe(
      'done (exited)',
    );
  });

  it('renders done (stopped) for an explicit stop', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'stopped' })).toBe(
      'done (stopped)',
    );
  });

  it('renders done (idle timeout) for a watchdog idle-timeout kill', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'idle_timeout' })).toBe(
      'done (idle timeout)',
    );
  });

  it('renders done (budget exceeded) for a budget breaker stop', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'budget_exceeded' })).toBe(
      'done (budget exceeded)',
    );
  });

  it('renders done (memory pressure) for a memory-pressure breaker stop', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'memory_pressure' })).toBe(
      'done (memory pressure)',
    );
  });

  it('renders bare done when status_reason is absent (defensive fallback)', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: null })).toBe('done');
  });

  it('falls back to done (stopped) for an unrecognized reason (forward-compat)', () => {
    expect(formatSessionStatus({ status: 'done', status_reason: 'something_new' })).toBe(
      'done (stopped)',
    );
  });
});

describe('isTerminal', () => {
  it('treats done and lost as terminal', () => {
    expect(isTerminal('done')).toBe(true);
    expect(isTerminal('lost')).toBe(true);
  });

  it('treats starting/working/waiting as non-terminal', () => {
    expect(isTerminal('starting')).toBe(false);
    expect(isTerminal('working')).toBe(false);
    expect(isTerminal('waiting')).toBe(false);
  });
});

describe('sessionStatusColor', () => {
  it('maps starting/working/waiting/lost to their dedicated colors', () => {
    expect(sessionStatusColor({ status: 'starting', status_reason: null })).toBe(
      'bg-status-creating',
    );
    expect(sessionStatusColor({ status: 'working', status_reason: null })).toBe('bg-status-active');
    expect(sessionStatusColor({ status: 'waiting', status_reason: null })).toBe('bg-status-idle');
    expect(sessionStatusColor({ status: 'lost', status_reason: null })).toBe('bg-status-lost');
  });

  it('renders done+exited as informational (green)', () => {
    expect(sessionStatusColor({ status: 'done', status_reason: 'exited' })).toBe('bg-status-ready');
  });

  it('renders done with any other reason (or absent) as warn (red)', () => {
    expect(sessionStatusColor({ status: 'done', status_reason: 'stopped' })).toBe(
      'bg-status-stopped',
    );
    expect(sessionStatusColor({ status: 'done', status_reason: 'idle_timeout' })).toBe(
      'bg-status-stopped',
    );
    expect(sessionStatusColor({ status: 'done', status_reason: null })).toBe('bg-status-stopped');
  });
});
