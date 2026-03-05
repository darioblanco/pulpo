import { describe, it, expect, vi, afterEach } from 'vitest';
import { cn, formatDuration } from './utils';

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
