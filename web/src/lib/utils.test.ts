import { describe, it, expect, vi, afterEach } from 'vitest';
import { cn, formatDuration, formatMemory, formatRelativeTime, statusColors } from './utils';

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

describe('statusColors', () => {
  it('has entries for all statuses', () => {
    expect(statusColors.active).toBe('bg-status-active');
    expect(statusColors.ready).toBe('bg-status-ready');
    expect(statusColors.stopped).toBe('bg-status-stopped');
    expect(statusColors.lost).toBe('bg-status-lost');
    expect(statusColors.creating).toBe('bg-status-creating');
    expect(statusColors.idle).toBe('bg-status-idle');
  });
});
