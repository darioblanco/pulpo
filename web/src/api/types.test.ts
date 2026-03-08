import { describe, it, expect } from 'vitest';
import { getProviderCapabilities } from './types';

describe('getProviderCapabilities', () => {
  it('returns full capabilities for claude', () => {
    const caps = getProviderCapabilities('claude');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(true);
    expect(caps.unrestricted).toBe(true);
  });

  it('returns limited capabilities for codex', () => {
    const caps = getProviderCapabilities('codex');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(false);
    expect(caps.unrestricted).toBe(false);
  });

  it('returns model support for gemini', () => {
    const caps = getProviderCapabilities('gemini');
    expect(caps.model).toBe(true);
    expect(caps.unrestricted).toBe(true);
  });

  it('returns no model support for open_code', () => {
    const caps = getProviderCapabilities('open_code');
    expect(caps.model).toBe(false);
  });

  it('returns no model support for opencode alias', () => {
    const caps = getProviderCapabilities('opencode');
    expect(caps.model).toBe(false);
  });

  it('returns full capabilities for unknown provider', () => {
    const caps = getProviderCapabilities('unknown');
    expect(caps.model).toBe(true);
    expect(caps.system_prompt).toBe(true);
  });
});
